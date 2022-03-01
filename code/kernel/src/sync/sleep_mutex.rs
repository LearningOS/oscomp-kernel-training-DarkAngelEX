use alloc::collections::LinkedList;

use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use super::mutex::SpinNoIrqLock;

struct SleepMutexSupport {
    pub locked: bool,
    pub queue: LinkedList<Waker>, // just for const new.
}

pub struct SleepMutex<T> {
    inner: SpinNoIrqLock<SleepMutexSupport>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T> Sync for SleepMutex<T> {}
unsafe impl<T> Send for SleepMutex<T> {}

pub struct SleepMutexGuard<'a, T> {
    mutex: &'a SleepMutex<T>,
}

unsafe impl<'a, T> Sync for SleepMutexGuard<'a, T> {}
unsafe impl<'a, T> Send for SleepMutexGuard<'a, T> {}

impl<T> SleepMutex<T> {
    pub const fn new(user_data: T) -> Self {
        Self {
            inner: SpinNoIrqLock::new(SleepMutexSupport {
                locked: false,
                queue: LinkedList::new(),
            }),
            data: UnsafeCell::new(user_data),
        }
    }
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let SleepMutex { data, .. } = self;
        data.into_inner()
    }
    pub async fn lock<'a>(&'a self) -> SleepMutexGuard<'a, T> {
        SleepMutexFuture { mutex: self }.await
    }
}
impl<'a, T> Deref for SleepMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<'a, T> DerefMut for SleepMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}
impl<'a, T> Drop for SleepMutexGuard<'a, T> {
    fn drop(&mut self) {
        let mut lock = self.mutex.inner.lock(place!());
        assert_eq!(lock.locked, true);
        lock.locked = false;
        if let Some(w) = lock.queue.pop_front() {
            w.wake();
        }
    }
}

struct SleepMutexFuture<'a, T> {
    mutex: &'a SleepMutex<T>,
}
impl<'a, T> Future for SleepMutexFuture<'a, T> {
    type Output = SleepMutexGuard<'a, T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut lock = self.mutex.inner.lock(place!());
        if lock.queue.is_empty() {
            lock.locked = true;
            return Poll::Ready(SleepMutexGuard { mutex: self.mutex });
        }
        lock.queue.push_back(cx.waker().clone());
        Poll::Pending
    }
}
