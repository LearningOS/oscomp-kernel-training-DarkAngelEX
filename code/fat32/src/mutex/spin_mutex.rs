#![allow(dead_code)]
use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

pub struct SpinMutex<T: ?Sized> {
    lock: AtomicBool,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized> Send for SpinMutex<T> {}
unsafe impl<T: ?Sized> Sync for SpinMutex<T> {}

struct MutexGuard<'a, T: ?Sized> {
    mutex: &'a SpinMutex<T>,
}
impl<'a, T: ?Sized> !Send for MutexGuard<'a, T> {}
impl<'a, T: ?Sized> !Sync for MutexGuard<'a, T> {}

impl<T> SpinMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicBool::new(false),
            data: UnsafeCell::new(data),
        }
    }
    /// rust中&mut意味着无其他引用 可以安全地获得内部引用
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }
    pub unsafe fn unsafe_get(&self) -> &T {
        &*self.data.get()
    }
    pub fn lock(&self) -> impl DerefMut<Target = T> + '_ {
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
        MutexGuard { mutex: self }
    }
}

impl<'a, T: ?Sized> Deref for MutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for MutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized> Drop for MutexGuard<'a, T> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        self.mutex.lock.store(false, Ordering::Release);
    }
}
