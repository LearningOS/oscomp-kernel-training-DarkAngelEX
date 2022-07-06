use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use ftl_util::{async_tools, list::ListNode};

use crate::{
    sync::mutex::SpinNoIrqLock,
    timer::{self, TimeTicks},
};

use super::WaitStatus;

struct Node {
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
    closed: bool,
}

impl FutexQueue {
    pub fn new() -> Self {
        Self {
            closed: false,
            list: ListNode::new(Node::new(0)),
        }
    }
    pub fn init(&mut self) {
        self.list.init();
    }
    pub fn closed(&self) -> bool {
        self.closed
    }
    fn push(&mut self, node: &mut ListNode<Node>) {
        self.list.push_prev(node);
    }
    #[inline]
    pub async fn wait(
        queue: &SpinNoIrqLock<FutexQueue>,
        mask: u32,
        timeout: TimeTicks,
        mut fail: impl FnMut() -> bool,
    ) -> WaitStatus {
        let mut future = FutexFuture {
            node: ListNode::new(Node::new(mask)),
            queue,
            timeout,
        };
        let mut ptr = unsafe { Pin::new_unchecked(&mut future) };
        ptr.as_mut().init().await;
        unsafe {
            let mut queue = queue.lock();
            if queue.closed {
                return WaitStatus::Closed;
            }
            // 临界区内检测数据
            if fail() {
                return WaitStatus::Fail;
            }
            queue.push(&mut ptr.as_mut().get_unchecked_mut().node);
        }
        timer::sleep::timer_push_task(timeout, async_tools::take_waker().await);
        ptr.await;
        WaitStatus::Ok
    }
    #[inline]
    pub fn wake(&mut self, mask: u32, n: usize) -> usize {
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
    /// 当futex被移除后唤醒全部线程
    #[inline]
    pub fn wake_all_close(&mut self) {
        self.closed = true;
        self.list.pop_many_when(
            usize::MAX,
            |_node| true,
            |node| {
                let waker = node.waker.take().unwrap();
                node.issue.store(true, Ordering::Release);
                waker.wake();
            },
        );
    }
}

struct FutexFuture<'a> {
    node: ListNode<Node>,
    queue: &'a SpinNoIrqLock<FutexQueue>,
    timeout: TimeTicks,
}

impl FutexFuture<'_> {
    #[inline]
    async fn init(mut self: Pin<&mut Self>) {
        unsafe {
            let queue = self.as_mut().get_unchecked_mut();
            queue.node.init();
            queue.node.data_mut().waker = Some(async_tools::take_waker().await);
        }
    }
}

impl Future for FutexFuture<'_> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.node.data().issue.load(Ordering::Acquire) {
            debug_assert!(self.node.is_empty());
            return Poll::Ready(());
        }
        let now = timer::get_time_ticks();
        if self.timeout > now {
            return Poll::Pending;
        }
        let _lk = self.queue.lock();
        if !self.node.data().issue.load(Ordering::Acquire) {
            unsafe { self.get_unchecked_mut().node.pop_self() };
        } else {
            debug_assert!(self.node.is_empty());
        }
        Poll::Ready(())
    }
}
