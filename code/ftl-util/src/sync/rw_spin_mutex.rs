#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{self, AtomicIsize, Ordering},
};

use super::MutexSupport;

pub struct RwSpinMutex<T: ?Sized, S: MutexSupport> {
    lock: AtomicIsize,
    _marker: PhantomData<S>,
    data: UnsafeCell<T>, // actual data
}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for RwSpinMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for RwSpinMutex<T, S> {}

impl<T, S: MutexSupport> RwSpinMutex<T, S> {
    /// Creates a new spinlock wrapping the supplied data.
    ///
    /// May be used statically:
    ///
    /// ```
    /// #![feature(const_fn)]
    /// use spin;
    ///
    /// static MUTEX: spin::Mutex<()> = spin::Mutex::new(());
    ///
    /// fn demo() {
    ///     let lock = MUTEX.lock();
    ///     // do something with lock
    ///     drop(lock);
    /// }
    /// ```
    pub const fn new(user_data: T) -> RwSpinMutex<T, S> {
        RwSpinMutex {
            lock: AtomicIsize::new(0),
            data: UnsafeCell::new(user_data),
            _marker: PhantomData,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let RwSpinMutex { data, .. } = self;
        data.into_inner()
    }
}

impl<T: ?Sized, S: MutexSupport> RwSpinMutex<T, S> {
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    pub fn try_unique_lock(&self) -> Option<impl DerefMut<Target = T> + '_> {
        let mut guard = S::before_lock();
        if self.lock.load(Ordering::Relaxed) != 0 {
            return None;
        }
        match self
            .lock
            .compare_exchange(0, -isize::MAX, Ordering::Acquire, Ordering::Relaxed)
        {
            Ok(_) => Some(UniqueRwMutexGuard { mutex: self, guard }),
            Err(_) => {
                S::after_unlock(&mut guard);
                None
            }
        }
    }
    pub fn unique_lock(&self) -> impl DerefMut<Target = T> + '_ {
        let guard = S::before_lock();
        let mut cnt = 0;
        loop {
            let cur = self.lock.load(Ordering::Relaxed);
            if cur != 0 {
                cnt += 1;
                core::hint::spin_loop();
                if cnt == 0x10000000 {
                    panic!("dead lock");
                }
                continue;
            }
            if self
                .lock
                .compare_exchange(0, -isize::MAX, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            return UniqueRwMutexGuard { mutex: self, guard };
        }
    }
    pub fn try_shared_lock(&self) -> Option<impl Deref<Target = T> + '_> {
        let mut guard = S::before_lock();
        let mut cur = self.lock.load(Ordering::Relaxed);
        while cur >= 0 {
            match self
                .lock
                .compare_exchange(cur, cur + 1, Ordering::Acquire, Ordering::Relaxed)
            {
                Ok(_) => return Some(SharedRwMutexGuard { mutex: self, guard }),
                Err(v) => cur = v,
            };
        }
        S::after_unlock(&mut guard);
        None
    }
    pub fn shared_lock(&self) -> impl Deref<Target = T> + '_ {
        let guard = S::before_lock();
        if self.lock.fetch_add(1, Ordering::Relaxed) >= 0 {
            atomic::fence(Ordering::Acquire);
            return SharedRwMutexGuard { mutex: self, guard };
        }
        let mut cnt = 0;
        while self.lock.load(Ordering::Relaxed) <= 0 {
            if cnt == 0x10000000 {
                panic!("dead lock");
            }
            cnt += 1;
            core::hint::spin_loop();
        }
        atomic::fence(Ordering::Acquire);
        return SharedRwMutexGuard { mutex: self, guard };
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for RwSpinMutex<T, S> {
    fn default() -> RwSpinMutex<T, S> {
        RwSpinMutex::new(Default::default())
    }
}

struct UniqueRwMutexGuard<'a, T: ?Sized, S: MutexSupport + 'a> {
    mutex: &'a RwSpinMutex<T, S>,
    guard: S::GuardData,
}
impl<'a, T: ?Sized, S: MutexSupport + 'a> !Send for UniqueRwMutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport + 'a> !Sync for UniqueRwMutexGuard<'a, T, S> {}

struct SharedRwMutexGuard<'a, T: ?Sized, S: MutexSupport + 'a> {
    mutex: &'a RwSpinMutex<T, S>,
    guard: S::GuardData,
}
impl<'a, T: ?Sized, S: MutexSupport + 'a> !Send for SharedRwMutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport + 'a> !Sync for SharedRwMutexGuard<'a, T, S> {}

impl<'a, T: ?Sized, S: MutexSupport + 'a> Deref for SharedRwMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<'a, T: ?Sized, S: MutexSupport + 'a> Deref for UniqueRwMutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport + 'a> DerefMut for UniqueRwMutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport + 'a> Drop for SharedRwMutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        let prev = self.mutex.lock.fetch_sub(1, Ordering::Release);
        debug_assert!(prev > 0);
        S::after_unlock(&mut self.guard);
    }
}
impl<'a, T: ?Sized, S: MutexSupport + 'a> Drop for UniqueRwMutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        debug_assert!(self.mutex.lock.load(Ordering::Relaxed) < 0);
        // debug_assert_eq!(self.mutex.lock.load(Ordering::Relaxed), -1);
        // self.mutex.lock.store(0, Ordering::Release);
        self.mutex.lock.fetch_add(isize::MAX, Ordering::Release);
        S::after_unlock(&mut self.guard);
    }
}
