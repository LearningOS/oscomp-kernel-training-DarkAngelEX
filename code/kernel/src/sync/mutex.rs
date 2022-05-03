#![allow(dead_code)]

use core::{
    cell::UnsafeCell,
    fmt,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

use ftl_util::sync::{MutexSupport, Spin};

use crate::{hart::cpu, timer};

use super::SpinNoIrq;

pub type SpinLock<T> = Mutex<T, Spin>;
pub type SpinNoIrqLock<T> = Mutex<T, SpinNoIrq>;

pub struct Mutex<T: ?Sized, S: MutexSupport> {
    lock: AtomicBool,
    user: UnsafeCell<(usize, usize)>, // (cid, tid)
    _marker: PhantomData<S>,
    data: UnsafeCell<T>, // actual data
}

struct MutexGuard<'a, T: ?Sized, S: MutexSupport + 'a> {
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
            data: UnsafeCell::new(user_data),
            _marker: PhantomData,
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
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub unsafe fn unsafe_get_mut(&self) -> &mut T {
        unsafe { &mut *self.data.get() }
    }
    #[inline(always)]
    fn obtain_lock(&self, place: &'static str) {
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            let mut try_count = 0usize;
            let start = timer::get_time_ticks();
            // Wait until the lock looks unlocked before retrying
            while self.lock.load(Ordering::Relaxed) {
                core::hint::spin_loop();
                try_count += 1;
                if try_count == 0x10000000 {
                    let now = timer::get_time_ticks();
                    let ms = (now - start).into_millisecond();
                    let (cid, tid) = unsafe { *self.user.get() };
                    panic!(
                        "Mutex: deadlock detected!\n\
                        - - spend {}ms(try_count > {:#x}) in {}\n\
                        - - locked by cpu {} thread {} @ {:?}",
                        ms, try_count, place, cid, tid, self as *const Self
                    );
                }
            }
        }
        let cid = cpu::hart_id();
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
    pub fn lock(&self, place: &'static str) -> impl DerefMut<Target = T> + '_ {
        let support_guard = S::before_lock();
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
    pub fn busy_lock(&self) -> impl DerefMut<Target = T> + '_ {
        loop {
            if let Some(x) = self.try_lock() {
                break x;
            }
            //yield_now();
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
    pub fn try_lock(&self) -> Option<impl DerefMut<Target = T> + '_> {
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

impl<T: ?Sized + fmt::Debug, S: MutexSupport> fmt::Debug for Mutex<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.try_lock() {
            Some(guard) => write!(f, "Mutex {{ data: {:?} }}", &*guard),
            None => write!(f, "Mutex {{ <locked> }}"),
        }
    }
}

impl<T: ?Sized + ~const Default, S: MutexSupport> const Default for Mutex<T, S> {
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
        S::after_unlock(&mut self.support_guard);
    }
}
