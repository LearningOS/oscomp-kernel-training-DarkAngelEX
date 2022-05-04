use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use alloc::sync::Arc;

use crate::{async_tools, list::ListNode};

use super::{spin_mutex::SpinMutex, MutexSupport};

type ListNodeImpl = ListNode<(bool, isize, Option<Waker>)>;

/// 此信号量生成的guard通过Arc维护,可以发送至'static闭包
pub struct Semaphore<S: MutexSupport> {
    inner: Arc<SpinMutex<SemaphoreInner, S>>,
}

unsafe impl<S: MutexSupport> Send for Semaphore<S> {}
unsafe impl<S: MutexSupport> Sync for Semaphore<S> {}

/// inner使用Arc包围
struct SemaphoreInner {
    cur: isize,
    max: isize,
    queue: ListNodeImpl,
}

impl Drop for SemaphoreInner {
    fn drop(&mut self) {
        debug_assert!(self.cur == self.max);
    }
}

/// 获取1个信号量
pub struct SemaphoreGuard<S: MutexSupport> {
    inner: MultiplySemaphore<S>,
}

/// 获取多个信号量
pub struct MultiplySemaphore<S: MutexSupport> {
    val: isize,
    ptr: Arc<SpinMutex<SemaphoreInner, S>>,
}

impl<S: MutexSupport> Drop for SemaphoreGuard<S> {
    fn drop(&mut self) {
        debug_assert!(self.inner.val == 1);
    }
}

impl<S: MutexSupport> Drop for MultiplySemaphore<S> {
    fn drop(&mut self) {
        if self.val != 0 {
            let inner = &mut *self.ptr.lock();
            inner.cur += self.val as isize;
            inner.release_task();
        }
    }
}
impl<S: MutexSupport> SemaphoreGuard<S> {
    pub fn into_multiply(self) -> MultiplySemaphore<S> {
        unsafe { core::mem::transmute(self) }
    }
}
impl<S: MutexSupport> MultiplySemaphore<S> {
    pub fn val(&self) -> usize {
        self.val as usize
    }
    pub fn try_take(&mut self) -> Option<SemaphoreGuard<S>> {
        debug_assert!(self.val >= 0);
        if self.val == 0 {
            return None;
        }
        self.val -= 1;
        Some(SemaphoreGuard {
            inner: Self {
                val: 1,
                ptr: self.ptr.clone(),
            },
        })
    }
}

impl SemaphoreInner {
    fn release_task(&mut self) {
        while let Some(mut p) = self.queue.try_next() {
            let next = unsafe { p.as_mut().data_mut() };
            if self.cur < next.1 {
                return;
            }
            unsafe { p.as_mut().pop_self() };
            self.cur -= next.1;
            let waker = next.2.take().unwrap();
            super::seq_fence();
            next.0 = true;
            waker.wake();
        }
    }
}

impl<S: MutexSupport> Semaphore<S> {
    pub fn new(n: usize) -> Self {
        debug_assert!(n <= isize::MAX as usize);
        let n = n as isize;
        Self {
            inner: Arc::new(SpinMutex::new(SemaphoreInner {
                cur: n,
                max: n,
                queue: ListNode::new((false, 0, None)),
            })),
        }
    }
    pub fn max(&self) -> usize {
        let v = unsafe { self.inner.unsafe_get().max };
        debug_assert!(v >= 0);
        v as usize
    }
    pub fn change(&self, n: isize) {
        let mut inner = self.inner.lock();
        inner.cur += n;
        inner.max += n;
        debug_assert!(inner.max >= 0);
        if n > 0 {
            inner.release_task();
        }
    }
    /// 获取一个信号量
    pub async fn take(&self) -> SemaphoreGuard<S> {
        SemaphoreGuard {
            inner: self.take_n(1).await,
        }
    }
    /// 获取一个信号量
    pub async fn take_n(&self, n: usize) -> MultiplySemaphore<S> {
        debug_assert!(n <= isize::MAX as usize);
        let future = &mut SemaphoreFuture::new(n as isize, self);
        unsafe { Pin::new_unchecked(future).init().await.await }
    }
}

struct SemaphoreFuture<'a, S: MutexSupport> {
    val: isize,
    sem: &'a Semaphore<S>,
    node: ListNodeImpl,
}

impl<'a, S: MutexSupport> SemaphoreFuture<'a, S> {
    fn new(val: isize, sem: &'a Semaphore<S>) -> Self {
        Self {
            val,
            sem,
            node: ListNodeImpl::new((false, val, None)),
        }
    }
    async fn init(self: Pin<&mut Self>) -> Pin<&mut SemaphoreFuture<'a, S>> {
        let this = unsafe { Pin::get_unchecked_mut(self) };
        this.node.init();
        let mut sem = unsafe { this.sem.inner.send_lock() };
        sem.queue.lazy_init();
        let data = this.node.data_mut();
        if sem.queue.is_empty() && sem.cur >= this.val {
            sem.cur -= this.val;
            data.0 = true;
        } else {
            data.2 = Some(async_tools::take_waker().await);
            sem.queue.push_prev(&mut this.node);
        }
        unsafe { Pin::new_unchecked(this) }
    }
}

impl<'a, S: MutexSupport> Future for SemaphoreFuture<'a, S> {
    type Output = MultiplySemaphore<S>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.node.data().0 {
            false => Poll::Pending,
            true => Poll::Ready(MultiplySemaphore {
                val: self.val,
                ptr: self.sem.inner.clone(),
            }),
        }
    }
}
