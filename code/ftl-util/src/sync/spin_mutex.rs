#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::async_tools::SendWraper;

use super::MutexSupport;

pub struct SpinMutex<T: ?Sized, S: MutexSupport> {
    lock: AtomicBool,
    _marker: PhantomData<S>,
    data: UnsafeCell<T>, // actual data
}

struct MutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a SpinMutex<T, S>,
    support_guard: S::GuardData,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
impl<'a, T: ?Sized, S: MutexSupport> !Sync for MutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Send for MutexGuard<'a, T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for SpinMutex<T, S> {}
unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for SpinMutex<T, S> {}

impl<T, S: MutexSupport> SpinMutex<T, S> {
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
    pub const fn new(user_data: T) -> Self {
        SpinMutex {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(user_data),
            _marker: PhantomData,
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let SpinMutex { data, .. } = self;
        data.into_inner()
    }
}

impl<T: ?Sized, S: MutexSupport> SpinMutex<T, S> {
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        &mut *self.data.get()
    }
    #[inline(always)]
    fn obtain_lock(&self) {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            let mut try_count = 0usize;
            // Wait until the lock looks unlocked before retrying
            while self.lock.load(Ordering::Relaxed) {
                core::hint::spin_loop();
                try_count += 1;
                if try_count == 0x10000000 {
                    panic!("Mutex: deadlock detected! try_count > {:#x}\n", try_count);
                }
            }
        }
    }
    /// Assume the mutex is free and get reference of value.
    ///
    /// This is only safe during initialization
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn assert_unique_get(&self) -> &mut T {
        assert!(!self.lock.load(Ordering::Relaxed));
        &mut *self.data.get()
    }

    /// Locks the spinlock and returns a guard.
    ///
    /// The returned value may be dereferenced for data access
    /// and the lock will be dropped when the guard falls out of scope.
    ///
    /// ```
    /// let mylock = spin::Mutex::new(0);
    /// {
    ///     let mut data = mylock.lock();
    ///     // The lock is now locked and the data can be accessed
    ///     *data += 1;
    ///     // The lock is implicitly dropped
    /// }
    ///
    /// ```
    #[inline(always)]
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        let support_guard = S::before_lock();
        self.obtain_lock();
        MutexGuard {
            mutex: self,
            support_guard,
        }
    }
    pub unsafe fn send_lock(&self) -> impl DerefMut<Target = T> + Send + '_ {
        SendWraper::new(self.lock())
    }
    pub fn get_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// Force unlock the spinlock.
    ///
    /// This is *extremely* unsafe if the lock is not held by the current
    /// thread. However, this can be useful in some instances for exposing the
    /// lock to FFI that doesn't know how to deal with RAII.
    ///
    /// If the lock isn't held, this is a no-op.
    pub unsafe fn force_unlock(&self) {
        self.lock.store(false, Ordering::Release);
    }

    /// Tries to lock the mutex. If it is already locked, it will return None. Otherwise it returns
    /// a guard within Some.
    pub fn try_lock(&self) -> Option<impl DerefMut<Target = T> + '_> {
        let mut support_guard = S::before_lock();
        if self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            Some(MutexGuard {
                mutex: self,
                support_guard,
            })
        } else {
            S::after_unlock(&mut support_guard);
            None
        }
    }
}

impl<T: ?Sized + fmt::Debug, S: MutexSupport> fmt::Debug for SpinMutex<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.try_lock() {
            Some(guard) => write!(f, "Mutex {{ data: {:?} }}", &*guard),
            None => write!(f, "Mutex {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for SpinMutex<T, S> {
    fn default() -> SpinMutex<T, S> {
        SpinMutex::new(Default::default())
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Deref for MutexGuard<'a, T, S> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> DerefMut for MutexGuard<'a, T, S> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized, S: MutexSupport> Drop for MutexGuard<'a, T, S> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        debug_assert!(self.mutex.lock.load(Ordering::Relaxed));
        self.mutex.lock.store(false, Ordering::Release);
        S::after_unlock(&mut self.support_guard);
    }
}
