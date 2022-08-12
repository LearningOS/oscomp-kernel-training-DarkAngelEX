use core::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::boxed::Box;

struct SharedBuffer(AtomicUsize);

impl SharedBuffer {
    /// 返回旧的值
    pub fn increase(&self) -> usize {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
    /// 返回旧的值
    pub fn decrease(&self) -> usize {
        let v = self.0.load(Ordering::Relaxed);
        debug_assert!(v != 0);
        if v != 1 {
            self.0.fetch_sub(1, Ordering::Relaxed)
        } else {
            self.0.store(0, Ordering::Relaxed);
            1
        }
    }
}

/// 一个多线程共享计数器, 只能手动释放
pub struct SharedCounter(NonNull<SharedBuffer>);

impl Drop for SharedCounter {
    fn drop(&mut self) {
        panic!("SharedCount must be released manually")
    }
}

unsafe impl Send for SharedCounter {}
unsafe impl Sync for SharedCounter {}

impl SharedCounter {
    pub fn new() -> Self {
        let ptr = Box::into_raw(Box::new(SharedBuffer(AtomicUsize::new(1))));
        unsafe { Self(NonNull::new_unchecked(ptr)) }
    }
    pub fn new_dup() -> (Self, Self) {
        let ptr = Box::into_raw(Box::new(SharedBuffer(AtomicUsize::new(2))));
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        (Self(ptr), Self(ptr))
    }
    fn buffer(&self) -> &SharedBuffer {
        unsafe { self.0.as_ref() }
    }
    /// 递减引用计数, 如果这是最后一个, 返回true
    #[must_use]
    pub fn consume(self) -> bool {
        let n = self.buffer().decrease();
        debug_assert_ne!(n, 0);
        let release = n == 1;
        if release {
            unsafe { Box::from_raw(self.0.as_ptr()) };
        }
        core::mem::forget(self);
        release
    }
    pub fn unique(&self) -> bool {
        self.buffer().0.load(Ordering::Relaxed) == 1
    }
}

impl Clone for SharedCounter {
    fn clone(&self) -> Self {
        self.buffer().increase();
        Self(self.0)
    }
}
