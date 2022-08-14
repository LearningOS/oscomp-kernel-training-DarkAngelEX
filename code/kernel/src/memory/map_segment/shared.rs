//! 此模块用来处理共享映射页

use core::{
    marker::PhantomData,
    ops::DerefMut,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{boxed::Box, vec::Vec};

use crate::{memory::address::PhyAddrRef4K, sync::mutex::SpinLock};

/// 包含原子计数的共享内存
struct SharedBuffer(AtomicUsize);

impl SharedBuffer {
    #[inline(always)]
    pub unsafe fn increase_single_thread(&self) -> usize {
        let old = self.0.load(Ordering::Relaxed);
        self.0.store(old + 1, Ordering::Relaxed);
        old
    }
    #[inline(always)]
    pub unsafe fn decrease_single_thread(&self) -> usize {
        let old = self.0.load(Ordering::Relaxed);
        self.0.store(old - 1, Ordering::Relaxed);
        old
    }
    /// 返回旧的值
    #[inline(always)]
    pub fn increase(&self) -> usize {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
    /// 返回旧的值
    #[inline(always)]
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

/// 计数器共享所有权句柄, 只能手动释放
pub struct SharedCounter(NonNull<SharedBuffer>);

impl Drop for SharedCounter {
    fn drop(&mut self) {
        panic!("SharedCount must be released manually")
    }
}

unsafe impl Send for SharedCounter {}
unsafe impl Sync for SharedCounter {}

impl SharedCounter {
    #[inline(always)]
    pub fn new() -> Self {
        let ptr = Box::into_raw(Box::new(SharedBuffer(AtomicUsize::new(1))));
        unsafe { Self(NonNull::new_unchecked(ptr)) }
    }
    #[inline(always)]
    pub fn new_dup() -> (Self, Self) {
        let ptr = Box::into_raw(Box::new(SharedBuffer(AtomicUsize::new(2))));
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        (Self(ptr), Self(ptr))
    }
    #[inline(always)]
    fn buffer(&self) -> &SharedBuffer {
        unsafe { self.0.as_ref() }
    }
    /// 递减引用计数, 如果这是最后一个, 返回true
    #[must_use]
    #[inline(always)]
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
    #[inline(always)]
    pub fn unique(&self) -> bool {
        self.buffer().0.load(Ordering::Relaxed) == 1
    }
}

impl Clone for SharedCounter {
    #[inline(always)]
    fn clone(&self) -> Self {
        self.buffer().increase();
        Self(self.0)
    }
}

pub struct SharedPage {
    sc: SharedCounter,
    page: PhyAddrRef4K,
}

impl SharedPage {
    pub fn new(sc: SharedCounter, page: PhyAddrRef4K) -> Self {
        Self { sc, page }
    }
}

/// 增加原子计数
pub struct IncreaseCache(Vec<SharedPage>);

impl IncreaseCache {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn push(&mut self, page: SharedPage) {
        self.0.push(page)
    }
    pub fn append(&mut self, src: &mut Self) {
        self.0.append(&mut src.0)
    }
    pub fn flush(&mut self, _guard: &mut SharedGuard) {
        for page in self.0.drain(..) {
            let old = unsafe { page.sc.buffer().increase_single_thread() };
            debug_assert!(old != 0);
        }
    }
}

/// 递减原子计数
pub struct DecreaseCache(Vec<SharedPage>);

impl DecreaseCache {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn push(&mut self, page: SharedPage) {
        self.0.push(page)
    }
    pub fn append(&mut self, src: &mut Self) {
        self.0.append(&mut src.0)
    }
    pub fn flush(&mut self, _guard: &mut SharedGuard, mut release: impl FnMut(PhyAddrRef4K)) {
        for page in self.0.drain(..) {
            let old = unsafe { page.sc.buffer().decrease_single_thread() };
            debug_assert!(old != 0);
            if old == 1 {
                release(page.page);
            }
        }
    }
}

pub struct SharedGuard {
    _inner: PhantomData<()>, // 用来防止被外界创建
}

static SHARED_UPDATER: SpinLock<SharedGuard> = SpinLock::new(SharedGuard {
    _inner: PhantomData,
});

/// 全局原子计数更新队列, 按序处理
///
/// 对值不为1的引用计数必须持有`SharedGuard`才能释放
///
/// 总是先递增原子计数, 再递减原子计数
pub fn lock_updater() -> impl DerefMut<Target = SharedGuard> {
    SHARED_UPDATER.lock()
}

pub fn try_lock_updater() -> Option<impl DerefMut<Target = SharedGuard>> {
    SHARED_UPDATER.try_lock()
}
