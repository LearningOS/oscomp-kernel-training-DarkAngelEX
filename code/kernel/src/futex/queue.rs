use core::{
    future::Future,
    pin::Pin,
    ptr::NonNull,
    sync::atomic::{AtomicPtr, Ordering},
    task::{Context, Poll, Waker},
};

use alloc::boxed::Box;
use ftl_util::{async_tools, list::ListNode, time::Instant};

use crate::{process::Pid, timer::sleep::TimeoutFuture};

use super::{Futex, WaitStatus, WakeStatus};

struct Node {
    mask: u32,
    futex: ViewFutex,
    waker: Option<Waker>,
    pid: Option<Pid>,
}

impl Node {
    pub fn new(mask: u32, pid: Option<Pid>) -> Self {
        Self {
            mask,
            waker: None,
            pid,
            futex: ViewFutex::new(),
        }
    }
}

/// Futex等待队列, 使用侵入式链表避免内存分配
///
/// 如果转移过程失败了会唤醒全部线程
pub struct FutexQueue {
    list: ListNode<Node>,
    closed: bool,
}

impl FutexQueue {
    #[inline]
    pub fn new() -> Self {
        Self {
            closed: false,
            list: ListNode::new(Node::new(0, None)),
        }
    }
    #[inline]
    pub fn init(&mut self) {
        self.list.init();
    }
    #[inline]
    pub fn closed(&self) -> bool {
        self.closed
    }
    #[inline]
    fn push(&mut self, node: &mut ListNode<Node>) {
        stack_trace!();
        self.list.push_prev(node);
    }
    /// 等待被另一个线程唤醒, 它会将当前线程在futex上的节点设为Issued
    #[inline]
    pub async fn wait(
        futex: &Futex,
        mask: u32,
        timeout: Instant,
        pid: Option<Pid>,
        fail: impl FnOnce() -> bool,
    ) -> WaitStatus {
        stack_trace!();
        debug_assert!(mask != 0);
        let mut future;
        let mut ptr;
        let waker = async_tools::take_waker().await;
        {
            let mut queue = futex.queue.lock();
            if queue.closed {
                return WaitStatus::Closed;
            }
            // 临界区内检测数据
            if fail() {
                return WaitStatus::Fail;
            }
            future = FutexFuture {
                node: ListNode::new(Node::new(mask, pid)),
            };
            ptr = unsafe { Pin::new_unchecked(&mut future) };
            ptr.as_mut().init(futex, &mut *queue, waker.clone());
        }
        TimeoutFuture::new(timeout, ptr.as_mut()).await;
        ptr.detach();
        WaitStatus::Ok
    }
    #[inline]
    pub fn wake(
        &mut self,
        mask: u32,
        max: usize,
        pid: Option<Pid>,
        fail: impl FnOnce() -> bool,
    ) -> WakeStatus {
        stack_trace!();
        if self.closed() {
            return WakeStatus::Closed;
        }
        if fail() {
            return WakeStatus::Fail;
        }
        let n = self.list.pop_many_when(
            max,
            |node| match (pid, node.pid) {
                (Some(x), Some(y)) if x != y => false,
                _ => node.mask & mask != 0,
            },
            |node| {
                let waker = node.waker.take().unwrap();
                node.futex.set_issued();
                waker.wake();
            },
        );
        WakeStatus::Ok(n)
    }
    #[inline]
    pub fn wake_requeue(
        &mut self,
        max_wake: usize,
        max_requeue: usize,
        pid: Option<Pid>,
        mut fail: impl FnMut() -> bool,
    ) -> (WakeStatus, Option<TempQueue>) {
        stack_trace!();
        if self.closed() {
            return (WakeStatus::Closed, None);
        }
        if fail() {
            return (WakeStatus::Fail, None);
        }
        let need_pop = move |node: &Node| !matches!((pid, node.pid), (Some(x), Some(y)) if x != y);
        let mut n = 0;
        let mut tq = TempQueue::new();
        let total = self
            .list
            .pop_many_ex(max_wake + max_requeue, need_pop, |node| {
                if n < max_wake {
                    let waker = node.data_mut().waker.take().unwrap();
                    node.data_mut().futex.set_issued();
                    waker.wake();
                    n += 1;
                } else {
                    node.data_mut().futex.set_waited();
                    tq.push(node);
                }
            });
        debug_assert!(n <= max_wake);
        if total == n {
            debug_assert!(tq.is_empty());
            (WakeStatus::Ok(n), None)
        } else {
            debug_assert!(!tq.is_empty());
            (WakeStatus::Ok(n), Some(tq))
        }
    }

    pub fn append(&mut self, q: &mut TempQueue, futex: &Futex) -> Result<(), ()> {
        if self.closed() {
            return Err(());
        }
        q.append_to(&mut self.list, |node| node.futex.set_queued(futex));
        Ok(())
    }

    /// 当futex被移除后唤醒全部线程
    #[inline]
    pub fn wake_all_close(&mut self) {
        stack_trace!();
        self.closed = true;
        self.list.pop_many_when(
            usize::MAX,
            |_node| true,
            |node| {
                let waker = node.waker.take().unwrap();
                node.futex.set_issued();
                waker.wake();
            },
        );
    }
}

pub struct TempQueue(Box<ListNode<Node>>);

impl Drop for TempQueue {
    fn drop(&mut self) {
        self.0.pop_many_when(
            usize::MAX,
            |_node| true,
            |node| {
                debug_assert!(matches!(node.futex.fetch(), ViewOp::Waited));
                node.futex.set_issued();
            },
        );
    }
}

impl TempQueue {
    pub fn new() -> Self {
        let mut v = Box::new(ListNode::new(Node::new(0, None)));
        v.init();
        Self(v)
    }
    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    fn push(&mut self, node: &mut ListNode<Node>) {
        debug_assert!(matches!(node.data().futex.fetch(), ViewOp::Waited));
        self.0.push_prev(node);
    }
    /// 将自身链表增加到 head 的 prev 端
    fn append_to(&mut self, head: &mut ListNode<Node>, mut run: impl FnMut(&mut Node)) {
        debug_assert!(!self.is_empty());
        debug_assert!(matches!(head.data().futex.fetch(), ViewOp::Waited));
        unsafe {
            let this_start = self.0.get_next();
            let this_end = self.0.get_prev();
            let head_end = head.get_prev();
            (*head_end).set_next(this_start);
            (*this_start).set_prev(head_end);
            (*this_end).set_next(head);
            head.set_prev(this_end);
            self.0.init();
            let mut cur = this_start;
            while cur != head {
                run((*cur).data_mut());
                cur = (*cur).get_next();
            }
        }
    }
}

/// 被等待器持有的futex指针
///
///     0 -> 已发射
///     dangling -> 等待
///     Other -> 存在队列
///
/// 转移的过程:
///    ---A---    >>>>    ---B---
///    | lock |          | lock |
/// ptr:  A -->-- wait -->-- B
///
struct ViewFutex(AtomicPtr<Futex>);

enum ViewOp {
    Issued,
    Queued(*mut Futex),
    Waited,
}

impl ViewFutex {
    pub fn new() -> Self {
        Self(AtomicPtr::new(Self::WAITED_V))
    }
    const ISSUED_V: *mut Futex = core::ptr::null_mut();
    const WAITED_V: *mut Futex = NonNull::<Futex>::dangling().as_ptr();
    #[inline]
    fn fetch(&self) -> ViewOp {
        let p = self.0.load(Ordering::Relaxed);
        if p == Self::ISSUED_V {
            ViewOp::Issued
        } else if p != Self::WAITED_V {
            ViewOp::Queued(p)
        } else {
            ViewOp::Waited // unlikely
        }
    }
    pub fn set_issued(&self) {
        debug_assert!(!matches!(self.fetch(), ViewOp::Issued));
        self.0.store(Self::ISSUED_V, Ordering::Relaxed);
    }
    pub fn set_waited(&self) {
        debug_assert!(matches!(self.fetch(), ViewOp::Queued(_)));
        self.0.store(Self::WAITED_V, Ordering::Relaxed);
    }
    pub fn set_queued(&self, new: &Futex) {
        debug_assert!(matches!(self.fetch(), ViewOp::Waited));
        self.0.store(new as *const _ as *mut _, Ordering::Relaxed);
    }
    /// None: issued
    ///
    /// Some(p): queued
    #[inline]
    fn load_queue(&self) -> Option<*mut Futex> {
        #[cfg(debug_assertions)]
        let mut cnt = 0;
        loop {
            match self.fetch() {
                ViewOp::Issued => return None,
                ViewOp::Queued(p) => return Some(p),
                ViewOp::Waited => (),
            }
            #[cfg(debug_assertions)]
            {
                cnt += 1;
                assert!(cnt < 1000000, "ViewFutex deadlock");
            }
        }
    }
    /// 使用双重检查法获取锁并运行函数
    ///
    /// Ok(()): success lock
    ///
    /// Err(()): issued
    #[inline]
    pub fn lock_queue_run<T>(&self, f: impl FnOnce(&mut FutexQueue) -> T) -> Result<T, ()> {
        stack_trace!();
        // 猜测自己的队列指针是被使用的, 所有修改都要在获取锁的情况下进行!
        let mut p = self.load_queue().ok_or(())?;
        loop {
            let queue = &mut *unsafe { &*p }.queue.lock();
            let q = self.load_queue().ok_or(())?;
            if p != q {
                p = q;
                continue;
            }
            // 现在获取的锁和自身的队列是同一个
            debug_assert!(!queue.closed());
            return Ok(f(queue));
        }
    }
}

struct FutexFuture {
    node: ListNode<Node>,
}

impl FutexFuture {
    #[inline]
    fn init(mut self: Pin<&mut Self>, futex: &Futex, queue: &mut FutexQueue, waker: Waker) {
        stack_trace!();
        unsafe {
            let this = self.as_mut().get_unchecked_mut();
            this.node.init();
            this.node.data_mut().waker = Some(waker);
            this.node.data_mut().futex.set_queued(futex);

            queue.push(&mut this.node);
        }
    }
    /// 需要在结束后手动调用
    fn detach(self: Pin<&mut Self>) {
        let _ = self.node.data().futex.lock_queue_run(|_q| unsafe {
            self.node.pop_self_fast();
        });
        self.node.data().futex.set_issued();
    }
}

impl Future for FutexFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let ViewOp::Issued = self.node.data().futex.fetch() {
            return Poll::Ready(());
        }
        Poll::Pending
    }
}
