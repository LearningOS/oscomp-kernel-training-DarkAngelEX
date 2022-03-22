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
    id_alloc: NonZeroUsize,
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
                id_alloc: unsafe { NonZeroUsize::new_unchecked(1) },
            }),
            data: UnsafeCell::new(user_data),
        }
    }
    /// 当线程阻塞于此函数时不会再响应任何的信号。
    ///
    /// 睡眠锁保证严格按提交顺序解锁。
    ///
    /// TODO: 如果增加响应逻辑则需要清除队列数据并更新slot，避免进入死锁。
    ///
    pub async fn lock(&self) -> SleepMutexGuard<'_, T> {
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
        stack_trace!();
        let mut mutex = self.mutex.inner.lock(place!());
        assert!(mutex.locked && mutex.slot.is_none());
        mutex.locked = false;
        if let Some((cnt, w)) = mutex.queue.pop_front() {
            mutex.slot.replace(cnt);
            // drop(mutex); // just for efficiency
            w.wake();
        }
    }
}

struct SleepMutexFuture<'a, T> {
    mutex: &'a SleepMutex<T>,
    id: Option<NonZeroUsize>, // Using Option can delay allocation of id to polling, reduce one mutex operation.
}

impl<'a, T> SleepMutexFuture<'a, T> {
    pub fn new(mutex: &'a SleepMutex<T>) -> Self {
        SleepMutexFuture { mutex, id: None }
    }
}

impl<'a, T> Future for SleepMutexFuture<'a, T> {
    type Output = SleepMutexGuard<'a, T>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        let mut mutex = self.mutex.inner.lock(place!());
        let mut need_push_queue = false;
        let id = *self.id.get_or_insert_with(|| {
            need_push_queue = true;
            let id = mutex.id_alloc;
            mutex.id_alloc = id.checked_add(1).unwrap(); // overflow a century later
            id
        });
        let locked = mutex.locked;
        let slot = mutex.slot;
        let mut push_queue = || {
            if need_push_queue {
                mutex.queue.push_back((id, cx.waker().clone()));
            }
            Poll::Pending
        };
        if locked {
            debug_assert_eq!(slot, None);
            return push_queue();
        }
        // now mutex.locked is false.
        match slot {
            Some(slot_id) if slot_id != id => push_queue(),
            _ => {
                mutex.slot = None;
                mutex.locked = true;
                Poll::Ready(SleepMutexGuard { mutex: self.mutex })
            }
        }
    }
}
