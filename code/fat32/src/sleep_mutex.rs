use alloc::collections::LinkedList;

use core::{
    cell::UnsafeCell,
    future::Future,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use crate::mutex::{Mutex, MutexSupport};

struct SleepMutexSupport {
    locked: bool,
    queue: LinkedList<(NonZeroUsize, Waker)>, // just for const new.
    slot: Option<NonZeroUsize>,
    id_alloc: NonZeroUsize,
}

pub struct SleepMutex<T, S: MutexSupport> {
    inner: Mutex<SleepMutexSupport, S>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T, S: MutexSupport> Sync for SleepMutex<T, S> {}
unsafe impl<T, S: MutexSupport> Send for SleepMutex<T, S> {}

pub struct SleepMutexGuard<'a, T, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
}

// unsafe impl<'a, T> Sync for SleepMutexGuard<'a, T> {}
// unsafe impl<'a, T> Send for SleepMutexGuard<'a, T> {}

impl<T, S: MutexSupport> SleepMutex<T, S> {
    pub const fn new(user_data: T) -> Self {
        Self {
            inner: Mutex::new(SleepMutexSupport {
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
    // pub async fn lock(&self) -> SleepMutexGuard<'_, T, S> {
    pub async fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        SleepMutexFuture::new(self).await
    }
}
impl<'a, T, S: MutexSupport> Deref for SleepMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<'a, T, S: MutexSupport> DerefMut for SleepMutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T, S: MutexSupport> Drop for SleepMutexGuard<'a, T, S> {
    fn drop(&mut self) {
        stack_trace!();
        let mut mutex = self.mutex.inner.lock();
        assert!(mutex.locked && mutex.slot.is_none());
        mutex.locked = false;
        if let Some((cnt, w)) = mutex.queue.pop_front() {
            mutex.slot.replace(cnt);
            w.wake();
        }
    }
}

struct SleepMutexFuture<'a, T, S: MutexSupport> {
    mutex: &'a SleepMutex<T, S>,
    id: Option<NonZeroUsize>, // Using Option can delay allocation of id to polling, reduce one mutex operation.
}

impl<'a, T, S: MutexSupport> SleepMutexFuture<'a, T, S> {
    pub fn new(mutex: &'a SleepMutex<T, S>) -> Self {
        SleepMutexFuture { mutex, id: None }
    }
}

impl<'a, T, S: MutexSupport> Future for SleepMutexFuture<'a, T, S> {
    type Output = SleepMutexGuard<'a, T, S>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
        let mut mutex = self.mutex.inner.lock();
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
