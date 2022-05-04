use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::{async_tools, list::ListNode};

use super::{spin_mutex::SpinMutex, MutexSupport};

pub struct SleepMutex<T: ?Sized, S: MutexSupport> {
    lock: SpinMutex<MutexInner, S>, // push at prev, release at next
    data: UnsafeCell<T>,            // actual data
}

struct MutexInner {
    this_ptr: usize, // 检测睡眠锁移动并重置头节点
    status: bool,
    queue: ListNode<(bool, Option<Waker>)>,
}

impl MutexInner {
    const fn new() -> Self {
        Self {
            this_ptr: 0,
            status: false,
            queue: ListNode::new((false, None)),
        }
    }
    fn lazy_init(&mut self) {
        if self.this_ptr != self as *mut _ as usize {
            self.queue.init();
            self.this_ptr = self as *mut _ as usize;
        }
    }
}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for SleepMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for SleepMutex<T, S> {}

impl<T, S: MutexSupport> SleepMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        Self {
            lock: SpinMutex::new(MutexInner::new()),
            data: UnsafeCell::new(user_data),
        }
    }
    pub fn into_inner(self) -> T {
        let Self { data, .. } = self;
        data.into_inner()
    }
}
impl<T: ?Sized + Send, S: MutexSupport> SleepMutex<T, S> {
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    pub async fn lock(&self) -> impl DerefMut<Target = T> + Send + Sync + '_ {
        let future = &mut SleepLockFuture::new(self);
        unsafe { Pin::new_unchecked(future).init().await.await }
    }
}

struct SleepLockFuture<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
    node: ListNode<(bool, Option<Waker>)>,
}

impl<'a, T: ?Sized, S: MutexSupport> SleepLockFuture<'a, T, S> {
    fn new(mutex: &'a SleepMutex<T, S>) -> Self {
        SleepLockFuture {
            mutex,
            node: ListNode::new((false, None)),
        }
    }
    async fn init(self: Pin<&mut Self>) -> Pin<&mut SleepLockFuture<'a, T, S>> {
        let this = unsafe { self.get_unchecked_mut() };
        this.node.init();
        let inner = unsafe { &mut *this.mutex.lock.send_lock() };
        inner.lazy_init();
        let data = this.node.data_mut();
        if !inner.status {
            inner.status = true; // lock
            data.0 = true;
        } else {
            data.1 = Some(async_tools::take_waker().await);
            inner.queue.push_prev(&mut this.node);
        }
        unsafe { Pin::new_unchecked(this) }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Future for SleepLockFuture<'a, T, S> {
    type Output = SleepMutexGuard<'a, T, S>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let ptr = &self.node.data().0;
        match *ptr {
            false => Poll::Pending,
            true => Poll::Ready(SleepMutexGuard { mutex: self.mutex }),
        }
    }
}

struct SleepMutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
}

unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Send for SleepMutexGuard<'a, T, S> {}
unsafe impl<'a, T: ?Sized + Send, S: MutexSupport> Sync for SleepMutexGuard<'a, T, S> {}

impl<'a, T: ?Sized, S: MutexSupport> Deref for SleepMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for SleepMutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for SleepMutexGuard<'a, T, S> {
    fn drop(&mut self) {
        let mut inner = self.mutex.lock.lock();
        debug_assert!(inner.status);
        let next = match inner.queue.pop_next() {
            None => {
                inner.status = false; // unlock
                return;
            }
            Some(mut next) => unsafe { next.as_mut().data_mut() },
        };
        drop(inner);
        // waker必须在 next.0 = true 之前获取, 在这之后next将失效
        let waker = next.1.take().unwrap();
        next.0 = true;
        waker.wake();
    }
}
