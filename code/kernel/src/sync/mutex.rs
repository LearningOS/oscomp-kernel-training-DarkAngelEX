#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    fmt,
    mem::MaybeUninit,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use crate::{
    hart::{cpu::hart_id, interrupt},
    user::AutoSie,
};

pub type SpinLock<T> = Mutex<T, Spin>;
pub type SpinNoIrqLock<T> = Mutex<T, SpinNoIrq>;
// pub type SleepLock<T> = Mutex<T, Condvar>;

pub struct Mutex<T: ?Sized, S: MutexSupport> {
    pub lock: AtomicBool,
    _unused: usize,
    support: MaybeUninit<S>,
    support_initialization: AtomicU8, // 0 = uninitialized, 1 = initializing, 2 = initialized
    user: UnsafeCell<(usize, usize)>, // (cid, tid)
    data: UnsafeCell<T>,              // actual data
}

pub struct MutexGuard<'a, T: ?Sized, S: MutexSupport + 'a> {
    mutex: &'a Mutex<T, S>,
    support_guard: S::GuardData,
}
// 禁止Mutex跨越await导致死锁或无意义阻塞
impl<'a, T: ?Sized, S: MutexSupport> !Sync for MutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Send for MutexGuard<'a, T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Sync for Mutex<T, S> {}

unsafe impl<T: ?Sized + Send, S: MutexSupport> Send for Mutex<T, S> {}

impl<T, S: MutexSupport> Mutex<T, S> {
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
    pub const fn new(user_data: T) -> Mutex<T, S> {
        Mutex {
            lock: AtomicBool::new(false),
            _unused: 0,
            data: UnsafeCell::new(user_data),
            support: MaybeUninit::uninit(),
            support_initialization: AtomicU8::new(0),
            user: UnsafeCell::new((0, 0)),
        }
    }

    /// Consumes this mutex, returning the underlying data.
    pub fn into_inner(self) -> T {
        // We know statically that there are no outstanding references to
        // `self` so there's no need to lock.
        let Mutex { data, .. } = self;
        data.into_inner()
    }
}

impl<T: ?Sized, S: MutexSupport> Mutex<T, S> {
    #[inline(always)]
    fn obtain_lock(&self, place: &'static str) {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            let mut try_count = 0usize;
            // Wait until the lock looks unlocked before retrying
            while self.lock.load(Ordering::Relaxed) {
                unsafe { &*self.support.as_ptr() }.cpu_relax();
                try_count += 1;
                if try_count == 0x10000000 {
                    let (cid, tid) = unsafe { *self.user.get() };
                    let value = unsafe { *(&self.lock as *const _ as *const u8) as usize };
                    panic!(
                        "Mutex: deadlock detected! try_count > {:#x} in {}\n locked by cpu {} thread {} @ {:?} value {}",
                        try_count, place, cid, tid, self as *const Self, value
                    );
                }
            }
        }
        let cid = hart_id();
        //let tid = processor().tid_option().unwrap_or(0);
        let tid = 0;
        unsafe { self.user.get().write((cid, tid)) };
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
    pub fn lock(&self, place: &'static str) -> MutexGuard<T, S> {
        let support_guard = S::before_lock();

        self.ensure_support();

        self.obtain_lock(place);
        MutexGuard {
            mutex: self,
            support_guard,
        }
    }
    ///
    pub fn get_ptr(&self) -> *mut T {
        self.data.get()
    }

    /// lock using busy waiting
    pub fn busy_lock(&self) -> MutexGuard<T, S> {
        loop {
            if let Some(x) = self.try_lock() {
                break x;
            }
            //yield_now();
        }
    }

    #[inline(always)]
    pub fn ensure_support(&self) {
        let initialization = self.support_initialization.load(Ordering::Relaxed);
        if initialization == 2 {
            return;
        };
        if initialization == 1
            || self
                .support_initialization
                .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
        {
            // Wait for another thread to initialize
            while self.support_initialization.load(Ordering::Acquire) == 1 {
                core::hint::spin_loop();
            }
        } else {
            // My turn to initialize
            (unsafe { core::ptr::write(self.support.as_ptr() as *mut _, S::new()) });
            self.support_initialization.store(2, Ordering::Release);
        }
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
    pub fn try_lock(&self) -> Option<MutexGuard<T, S>> {
        let support_guard = S::before_lock();
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
            None
        }
    }
}

impl<T: ?Sized + fmt::Debug, S: MutexSupport + fmt::Debug> fmt::Debug for Mutex<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.try_lock() {
            Some(guard) => write!(
                f,
                "Mutex {{ data: {:?}, support: {:?} }}",
                &*guard, self.support
            ),
            None => write!(f, "Mutex {{ <locked>, support: {:?} }}", self.support),
        }
    }
}

impl<T: ?Sized + Default, S: MutexSupport> Default for Mutex<T, S> {
    fn default() -> Mutex<T, S> {
        Mutex::new(Default::default())
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
        unsafe { self.mutex.user.get().write((127, 127)) };
        self.mutex.lock.store(false, Ordering::Release);
        unsafe { &*self.mutex.support.as_ptr() }.after_unlock();
    }
}

/// Low-level support for mutex
pub trait MutexSupport {
    type GuardData;
    fn new() -> Self;
    /// Called when failing to acquire the lock
    fn cpu_relax(&self);
    /// Called before lock() & try_lock()
    fn before_lock() -> Self::GuardData;
    /// Called when MutexGuard dropping
    fn after_unlock(&self);
}

/// Spin lock
#[derive(Debug)]
pub struct Spin;

impl MutexSupport for Spin {
    type GuardData = ();

    fn new() -> Self {
        Spin
    }
    fn cpu_relax(&self) {
        core::hint::spin_loop();
    }
    fn before_lock() -> Self::GuardData {}
    fn after_unlock(&self) {}
}

/// Spin & no-interrupt lock
#[derive(Debug)]
pub struct SpinNoIrq;

/// Contains RFLAGS before disable interrupt, will auto restore it when dropping
pub struct FlagsGuard(bool);

impl Drop for FlagsGuard {
    fn drop(&mut self) {
        unsafe { interrupt::restore(self.0) };
    }
}

impl FlagsGuard {
    pub fn no_irq_region() -> Self {
        Self(unsafe { interrupt::disable_and_store() })
    }
}

impl MutexSupport for SpinNoIrq {
    type GuardData = FlagsGuard;
    fn new() -> Self {
        Self
    }
    fn cpu_relax(&self) {
        core::hint::spin_loop();
    }
    #[inline(always)]
    fn before_lock() -> Self::GuardData {
        FlagsGuard::no_irq_region()
    }
    fn after_unlock(&self) {}
}
// impl MutexSupport for SpinNoIrq {
//     type GuardData = AutoSie;
//     fn new() -> Self {
//         Self
//     }
//     fn cpu_relax(&self) {
//         core::hint::spin_loop();
//     }
//     #[inline(always)]
//     fn before_lock() -> Self::GuardData {
//         AutoSie::new()
//     }
//     fn after_unlock(&self) {}
// }

// impl MutexSupport for Condvar {
//     type GuardData = ();
//     fn new() -> Self {
//         Condvar::new()
//     }
//     fn cpu_relax(&self) {
//         self._wait();
//     }
//     fn before_lock() -> Self::GuardData {}
//     fn after_unlock(&self) {
//         self.notify_one();
//     }
// }
