//! 此模块用来处理共享映射页

use core::{
    marker::PhantomData,
    ops::DerefMut,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{boxed::Box, vec::Vec};

use crate::{memory::address::PhyAddrRef4K, sync::mutex::SpinLock};

/// 包含原子计数的共享内存, 保存了物理内存以便释放
struct SharedBuffer(AtomicUsize, PhyAddrRef4K);

impl SharedBuffer {
    /// 返回旧的值
    #[inline(always)]
    fn increase(&self) -> usize {
        let old = self.0.load(Ordering::Relaxed);
        self.0.store(old + 1, Ordering::Relaxed);
        old
    }
    /// 返回旧的值
    #[inline(always)]
    fn decrease(&self) -> usize {
        let old = self.0.load(Ordering::Relaxed);
        self.0.store(old - 1, Ordering::Relaxed);
        old
    }
}

/// 计数器共享所有权句柄, 只能手动释放
#[derive(Debug)]
pub struct SharedPage(NonNull<SharedBuffer>);

impl Drop for SharedPage {
    fn drop(&mut self) {
        panic!("SharedCount must be released manually")
    }
}

unsafe impl Send for SharedPage {}
unsafe impl Sync for SharedPage {}

impl SharedPage {
    // #[inline(always)]
    // pub fn new(pa: PhyAddrRef4K) -> Self {
    //     let ptr = Box::into_raw(Box::new(SharedBuffer(AtomicUsize::new(1), pa)));
    //     unsafe { Self(NonNull::new_unchecked(ptr)) }
    // }
    /// 一次性生成一个值为2的内存, 降低一次递增操作
    #[inline(always)]
    pub fn new_dup(pa: PhyAddrRef4K) -> (Self, Self) {
        let ptr = Box::into_raw(Box::new(SharedBuffer(AtomicUsize::new(2), pa)));
        let ptr = unsafe { NonNull::new_unchecked(ptr) };
        (Self(ptr), Self(ptr))
    }
    #[inline(always)]
    fn buffer(&self) -> &SharedBuffer {
        unsafe { self.0.as_ref() }
    }
    /// 引用计数为1时返回Ok(()), 由外界负责释放内存, buffer将立刻释放
    ///
    /// try_consume失败后应该立刻加入decrease集合
    #[must_use]
    pub fn try_consume(self) -> Result<(), Self> {
        if self.unique() {
            unsafe { Box::from_raw(self.0.as_ptr()) }; // 释放共享内存
            core::mem::forget(self);
            return Ok(());
        }
        Err(self)
    }
    /// 递减buffer引用计数, 如果引用计数减为0, 返回true并释放buffer
    ///
    /// 必须持有锁才能操作
    #[must_use]
    #[inline(always)]
    fn consume(self, _guard: &mut SharedGuard) -> Option<PhyAddrRef4K> {
        let n = self.buffer().decrease();
        debug_assert_ne!(n, 0);
        if n == 1 {
            let page = self.buffer().1;
            unsafe { Box::from_raw(self.0.as_ptr()) };
            core::mem::forget(self);
            Some(page)
        } else {
            core::mem::forget(self);
            None
        }
    }
    #[inline(always)]
    pub fn unique(&self) -> bool {
        self.buffer().0.load(Ordering::Relaxed) == 1
    }
    pub fn fork(&self, inc: &mut IncreaseCache) -> Self {
        if self.unique() {
            self.buffer().increase();
        } else {
            inc.push(Self(self.0));
        }
        Self(self.0)
    }
}

/// 增加原子计数, 在fork时被使用
pub struct IncreaseCache(Vec<SharedPage>);

impl IncreaseCache {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn push(&mut self, page: SharedPage) {
        self.0.push(page)
    }
    #[inline]
    pub fn flush(&mut self, _guard: &mut SharedGuard) {
        for sc in self.0.drain(..) {
            let old = sc.buffer().increase();
            debug_assert!(old != 0);
            core::mem::forget(sc);
        }
    }
}

/// 递减原子计数, 在page_fault或unmap中被使用
pub struct DecreaseCache(Vec<SharedPage>);

impl DecreaseCache {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn push(&mut self, page: SharedPage) {
        self.0.push(page)
    }
    #[inline]
    pub fn flush(&mut self, guard: &mut SharedGuard, mut release: impl FnMut(PhyAddrRef4K)) {
        for page in self.0.drain(..) {
            if let Some(pa) = page.consume(guard) {
                release(pa)
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

/// 全局原子计数更新锁, 通过降低并行度来显著提高吞吐量
///
/// 值不为1的引用计数必须持有`SharedGuard`锁才能释放
///
/// 总是先递增原子计数, 再递减原子计数
pub fn lock_updater() -> impl DerefMut<Target = SharedGuard> {
    SHARED_UPDATER.lock()
}

pub fn try_lock_updater() -> Option<impl DerefMut<Target = SharedGuard>> {
    SHARED_UPDATER.try_lock()
}
