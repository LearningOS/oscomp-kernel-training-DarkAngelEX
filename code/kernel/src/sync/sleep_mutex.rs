use alloc::collections::LinkedList;

use core::{
    cell::UnsafeCell,
    future::Future,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use super::mutex::SpinNoIrqLock;

struct SleepMutexSupport {
    locked: bool,
    queue: LinkedList<(NonZeroUsize, Waker)>, // just for const new.
    slot: Option<NonZeroUsize>,
    cnt: NonZeroUsize,
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

// unsafe impl<'a, T> Sync for SleepMutexGuard<'a, T> {}
// unsafe impl<'a, T> Send for SleepMutexGuard<'a, T> {}

impl<T> SleepMutex<T> {
    pub const fn new(user_data: T) -> Self {
        Self {
            inner: SpinNoIrqLock::new(SleepMutexSupport {
                locked: false,
                queue: LinkedList::new(),
                slot: None,
                cnt: unsafe { NonZeroUsize::new_unchecked(1) },
            }),
            data: UnsafeCell::new(user_data),
        }
    }
    /// 当线程阻塞于此函数时不会再响应任何的信号。
    ///
    /// TODO: 如果增加响应逻辑则需要清除队列数据并更新slot，避免进入死锁。
    ///
    pub async fn lock<'a>(&'a self) -> SleepMutexGuard<'a, T> {
        SleepMutexFuture::new(self).await
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
        let mut mutex = self.mutex.inner.lock(place!());
        assert_eq!(mutex.locked, true);
        mutex.locked = false;
        if let Some((cnt, w)) = mutex.queue.pop_front() {
            // assert mutex.slot is None.
            mutex.slot.replace(cnt).is_some().then(|| unreachable!());
            drop(mutex); // just for efficiency
            w.wake();
        }
    }
}

struct SleepMutexFuture<'a, T> {
    mutex: &'a SleepMutex<T>,
    id: Option<NonZeroUsize>,
}

impl<'a, T> SleepMutexFuture<'a, T> {
    pub fn new(mutex: &'a SleepMutex<T>) -> Self {
        SleepMutexFuture { mutex, id: None }
    }
}

impl<'a, T> Future for SleepMutexFuture<'a, T> {
    type Output = SleepMutexGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut mutex = self.mutex.inner.lock(place!());
        let id = if let Some(id) = self.id {
            id
        } else {
            let id = mutex.cnt;
            mutex.cnt = mutex.cnt.checked_add(1).unwrap(); // overflow a century later
            self.id = Some(id);
            id
        };
        if mutex.locked {
            debug_check_eq!(mutex.slot, None);
            mutex.queue.push_back((id, cx.waker().clone()));
            return Poll::Pending;
        }
        match mutex.slot {
            Some(slot_id) if slot_id != id => {
                mutex.queue.push_back((id, cx.waker().clone()));
                return Poll::Pending;
            }
            _ => {
                mutex.slot = None;
                mutex.locked = true;
                return Poll::Ready(SleepMutexGuard { mutex: self.mutex });
            }
        }
    }
}
