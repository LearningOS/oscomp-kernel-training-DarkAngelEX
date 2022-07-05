use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use ftl_util::list::ListNode;

use crate::{
    sync::mutex::SpinNoIrqLock,
    timer::{self, TimeTicks},
};

pub struct Node {
    mask: u32,
    issue: AtomicBool,
    waker: Option<Waker>,
}

impl Node {
    pub fn new(mask: u32) -> Self {
        Self {
            mask,
            waker: None,
            issue: AtomicBool::new(false),
        }
    }
}

pub struct FutexQueue {
    list: ListNode<Node>,
}

impl FutexQueue {
    pub fn new() -> Self {
        Self {
            list: ListNode::new(Node::new(0)),
        }
    }
    pub fn init(&mut self) {
        self.list.init();
    }
    pub fn push(&mut self, node: &mut ListNode<Node>) {
        self.list.push_prev(node);
    }
    pub fn pop_wake(&mut self, mask: u32, n: usize) -> usize {
        self.list.pop_many_when(
            n,
            |node| node.mask & mask != 0,
            |node| {
                let waker = node.waker.take().unwrap();
                node.issue.store(true, Ordering::Release);
                waker.wake();
            },
        )
    }
}

struct FutexFuture<'a> {
    node: ListNode<Node>,
    futex: &'a SpinNoIrqLock<FutexQueue>,
    timeout: TimeTicks,
}

impl Future for FutexFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.node.data().issue.load(Ordering::Acquire) {
            return Poll::Ready(());
        }
        let now = timer::get_time_ticks();
        if self.timeout > now {
            return Poll::Pending;
        }
        let _lk = self.futex.lock();
        if !self.node.data().issue.load(Ordering::Acquire) {
            unsafe {
                Pin::get_unchecked_mut(self).node.pop_self();
            }
        } else {
            debug_assert!(self.node.is_empty());
        }
        Poll::Ready(())
    }
}
