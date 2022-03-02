use alloc::collections::{BTreeSet, LinkedList};

use core::{
    cell::UnsafeCell,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, Waker},
};

use super::mutex::SpinNoIrqLock;

struct SleepMutexSupport {
    locked: bool,
    queue: LinkedList<Waker>, // just for const new.
    cnt: usize,
}

pub struct SleepMutex<T> {
    inner: SpinNoIrqLock<SleepMutexSupport>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T> Sync for SleepMutex<T> {}
unsafe impl<T> Send for SleepMutex<T> {}

pub struct SleepMutexGuard<'a, T> {
    mutex: &'a SleepMutex<T>,
    pub cnt: usize,
}

// unsafe impl<'a, T> Sync for SleepMutexGuard<'a, T> {}
// unsafe impl<'a, T> Send for SleepMutexGuard<'a, T> {}

impl<T> SleepMutex<T> {
    pub const fn new(user_data: T) -> Self {
        Self {
            inner: SpinNoIrqLock::new(SleepMutexSupport {
                locked: false,
                queue: LinkedList::new(),
                cnt: 0,
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

static CNT_DEALLOC_TRACE: SpinNoIrqLock<BTreeSet<usize>> = SpinNoIrqLock::new(BTreeSet::new());
static CNT_ALLOC_TRACE: SpinNoIrqLock<BTreeSet<usize>> = SpinNoIrqLock::new(BTreeSet::new());
static CNT_ALLOC_TRACE2: SpinNoIrqLock<BTreeSet<usize>> = SpinNoIrqLock::new(BTreeSet::new());

pub fn check_cnt(cnt: usize) {
    assert!(
        CNT_ALLOC_TRACE2.lock(place!()).insert(cnt),
        "lock.cnt: {}",
        cnt
    );
}

impl<'a, T> Drop for SleepMutexGuard<'a, T> {
    fn drop(&mut self) {
        let mut lock = self.mutex.inner.lock(place!());
        assert!(
            CNT_ALLOC_TRACE.lock(place!()).contains(&lock.cnt),
            "lock.cnt: {}",
            lock.cnt
        );
        assert!(
            CNT_ALLOC_TRACE2.lock(place!()).contains(&lock.cnt),
            "lock.cnt: {}",
            lock.cnt
        );
        assert!(
            CNT_DEALLOC_TRACE.lock(place!()).insert(lock.cnt),
            "lock.cnt: {}",
            lock.cnt
        );
        assert_eq!(lock.locked, true, "cnt: {} {}", self.cnt, lock.cnt);
        lock.locked = false;
        if let Some(w) = lock.queue.pop_front() {
            // drop(lock);
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
            lock.cnt += 1;
            assert!(
                CNT_ALLOC_TRACE.lock(place!()).insert(lock.cnt),
                "lock.cnt: {}",
                lock.cnt
            );
            return Poll::Ready(SleepMutexGuard {
                mutex: self.mutex,
                cnt: lock.cnt,
            });
        }
        lock.queue.push_back(cx.waker().clone());
        Poll::Pending
    }
}
