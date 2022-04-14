use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicIsize, Ordering},
};

/// 为了提高效率 不保证排他锁无饥饿
///
/// -1为排他锁 >0 为共享锁
pub struct RwSpinMutex<T: ?Sized> {
    lock: AtomicIsize,
    data: UnsafeCell<T>,
}

unsafe impl<T: ?Sized> Send for RwSpinMutex<T> {}
unsafe impl<T: ?Sized> Sync for RwSpinMutex<T> {}

struct UniqueRwMutexGuard<'a, T: ?Sized> {
    mutex: &'a RwSpinMutex<T>,
}
impl<'a, T: ?Sized> !Send for UniqueRwMutexGuard<'a, T> {}
impl<'a, T: ?Sized> !Sync for UniqueRwMutexGuard<'a, T> {}

struct SharedRwMutexGuard<'a, T: ?Sized> {
    mutex: &'a RwSpinMutex<T>,
}
impl<'a, T: ?Sized> !Send for SharedRwMutexGuard<'a, T> {}
impl<'a, T: ?Sized> !Sync for SharedRwMutexGuard<'a, T> {}

impl<T> RwSpinMutex<T> {
    pub const fn new(data: T) -> Self {
        Self {
            lock: AtomicIsize::new(0),
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
    pub fn unique_lock(&self) -> impl DerefMut<Target = T> + '_ {
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
                .compare_exchange(0, -1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            return UniqueRwMutexGuard { mutex: self };
        }
    }
    pub fn shared_lock(&self) -> impl Deref<Target = T> + '_ {
        let mut cnt = 0;
        loop {
            let cur = self.lock.load(Ordering::Relaxed);
            if cur < 0 {
                cnt += 1;
                core::hint::spin_loop();
                if cnt == 0x10000000 {
                    panic!("dead lock");
                }
                continue;
            }
            if self
                .lock
                .compare_exchange(cur, cur + 1, Ordering::Acquire, Ordering::Relaxed)
                .is_err()
            {
                continue;
            }
            return SharedRwMutexGuard { mutex: self };
        }
    }
}

impl<'a, T: ?Sized> Deref for SharedRwMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}
impl<'a, T: ?Sized> Deref for UniqueRwMutexGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized> DerefMut for UniqueRwMutexGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.mutex.data.get() }
    }
}

impl<'a, T: ?Sized> Drop for SharedRwMutexGuard<'a, T> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        let prev = self.mutex.lock.fetch_sub(1, Ordering::Release);
        debug_assert!(prev > 0);
    }
}
impl<'a, T: ?Sized> Drop for UniqueRwMutexGuard<'a, T> {
    /// The dropping of the MutexGuard will release the lock it was created from.
    fn drop(&mut self) {
        debug_assert_eq!(self.mutex.lock.load(Ordering::Relaxed), -1);
        self.mutex.lock.store(0, Ordering::Release);
    }
}
