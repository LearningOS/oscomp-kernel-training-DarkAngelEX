#![allow(dead_code)]
use core::{
    cell::UnsafeCell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct MutexGuard<'a, T: ?Sized, S: MutexSupport> {
    mutex: &'a Mutex<T, S>,
    support_guard: S::GuardData,
}
impl<'a, T: ?Sized, S: MutexSupport> !Send for MutexGuard<'a, T, S> {}
impl<'a, T: ?Sized, S: MutexSupport> !Sync for MutexGuard<'a, T, S> {}

pub struct Mutex<T: ?Sized, S: MutexSupport> {
    lock: AtomicBool,
    _marker: PhantomData<S>,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized, S: MutexSupport> Send for Mutex<T, S> {}
unsafe impl<T: ?Sized, S: MutexSupport> Sync for Mutex<T, S> {}

impl<T, S: MutexSupport> Mutex<T, S> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
            _marker: PhantomData,
        }
    }
    /// rust中&mut意味着无其他引用 可以安全地获得内部引用
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub fn lock(&self) -> MutexGuard<'_, T, S> {
        let support_guard = S::before_lock();
        while self
            .lock
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            let mut cnt = 0;
            while self.lock.load(Ordering::Relaxed) {
                core::hint::spin_loop();
                // 时间约为1s
                if cnt == 0x10000000 {
                    panic!("dead lock");
                }
                cnt += 1;
            }
        }
        MutexGuard {
            mutex: self,
            support_guard,
        }
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
        self.mutex.lock.store(false, Ordering::Release);
        S::after_lock(&self.support_guard);
    }
}

pub trait MutexSupport: 'static {
    type GuardData;
    fn before_lock() -> Self::GuardData;
    fn after_lock(v: &Self::GuardData);
}

pub struct Spin;
impl MutexSupport for Spin {
    type GuardData = ();
    fn before_lock() -> Self::GuardData {}
    fn after_lock(_v: &Self::GuardData) {}
}
