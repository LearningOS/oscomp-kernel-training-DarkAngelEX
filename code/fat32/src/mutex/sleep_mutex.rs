use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use alloc::collections::LinkedList;

use crate::tools::AID;

use super::spin_mutex::SpinMutex;

pub struct SleepMutex<T> {
    inner: SpinMutex<SleepMutexSupport>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T> Sync for SleepMutex<T> {}
unsafe impl<T> Send for SleepMutex<T> {}

struct SleepMutexSupport {
    pub alloc_id: AID,
    pub allow_id: AID,
    pub queue: LinkedList<(AID, Waker)>,
}

struct SleepMutexGuard<'a, T> {
    mutex: &'a SleepMutex<T>,
}

impl<T> Drop for SleepMutexGuard<'_, T> {
    fn drop(&mut self) {
        let mut inner = self.mutex.inner.lock();
        // let slot = inner.allow_id.step();
        inner.allow_id.step();
        if let Some((aid, w)) = inner.queue.pop_front() {
            debug_assert_eq!(inner.allow_id, aid);
            w.wake();
        }
    }
}

impl<T> SleepMutex<T> {
    pub const fn new(user_data: T) -> Self {
        Self {
            inner: SpinMutex::new(SleepMutexSupport {
                alloc_id: AID(1),
                allow_id: AID(1),
                queue: LinkedList::new(),
            }),
            data: UnsafeCell::new(user_data),
        }
    }
    /// rust中&mut意味着无其他引用 可以安全地获得内部引用
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    /// 保证按顺序解锁
    pub async fn lock(&self) -> impl DerefMut<Target = T> + Send + Sync + '_ {
        SleepMutexFuture::<'_, T> {
            mutex: self,
            id: AID(0),
        }
        .await
    }
}

struct SleepMutexFuture<'a, T> {
    mutex: &'a SleepMutex<T>,
    id: AID,
}

impl<T> Deref for SleepMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<T> DerefMut for SleepMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T> Future for SleepMutexFuture<'a, T> {
    type Output = SleepMutexGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if !self.id.is_zero() {
            if self.id == unsafe { self.mutex.inner.unsafe_get().allow_id } {
                return Poll::Ready(SleepMutexGuard { mutex: self.mutex });
            }
        }
        let mut inner = self.mutex.inner.lock();
        if self.id.is_zero() {
            self.id = inner.alloc_id.step();
            if self.id == inner.allow_id {
                return Poll::Ready(SleepMutexGuard { mutex: self.mutex });
            }
            inner.queue.push_back((self.id, cx.waker().clone()));
            return Poll::Pending;
        }
        if self.id == inner.allow_id {
            return Poll::Ready(SleepMutexGuard { mutex: self.mutex });
        }
        debug_assert!(self.id > inner.allow_id);
        Poll::Pending
    }
}
