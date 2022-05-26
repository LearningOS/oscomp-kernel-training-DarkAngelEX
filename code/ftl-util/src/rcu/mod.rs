use core::{
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    sync::atomic,
};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::sync::{spin_mutex::SpinMutex, MutexSupport, Spin};

pub mod manager;

/// 禁止跨越await
///
/// 引用可以防止被普通析构
pub struct RcuReadGuard<'a, T: RcuCollect> {
    value: ManuallyDrop<T>,
    _mark: PhantomData<&'a T>,
}

impl<'a, T: RcuCollect> !Send for RcuReadGuard<'a, T> {}
impl<'a, T: RcuCollect> !Sync for RcuReadGuard<'a, T> {}

impl<'a, T: RcuCollect> Deref for RcuReadGuard<'a, T> {
    type Target = T;
    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.value.deref()
    }
}

/// Rcu类型宽度不能超过usize, align必须和size一致
pub trait RcuCollect: Sized + 'static {
    fn rcu_assert() {
        use core::mem::{align_of, size_of};
        assert_eq!(size_of::<Self>(), align_of::<Self>());
        assert!(size_of::<Self>() <= size_of::<usize>());
        assert!(4 <= size_of::<usize>());
    }
    #[must_use]
    #[inline(always)]
    fn rcu_read(&self) -> RcuReadGuard<Self> {
        Self::rcu_assert();
        let value = unsafe { core::mem::ManuallyDrop::new(core::ptr::read_volatile(self)) };
        atomic::fence(atomic::Ordering::Acquire);
        RcuReadGuard {
            value,
            _mark: PhantomData,
        }
    }
    #[inline]
    unsafe fn rcu_write(&self, src: Self) {
        Self::rcu_assert();
        atomic::fence(atomic::Ordering::Release);
        core::ptr::replace(self as *const _ as *mut Self, src).rcu_drop();
    }
    #[must_use]
    #[inline(always)]
    unsafe fn rcu_into(self) -> usize {
        use core::mem::{size_of, transmute_copy};
        Self::rcu_assert();
        const USIZE_SIZE: usize = size_of::<usize>();
        let v = match size_of::<Self>() {
            1 => transmute_copy::<Self, u8>(&self) as usize,
            2 => transmute_copy::<Self, u16>(&self) as usize,
            4 => transmute_copy::<Self, u32>(&self) as usize,
            USIZE_SIZE => transmute_copy::<Self, usize>(&self),
            size => unreachable!("no support size: {}", size),
        };
        core::mem::forget(self);
        v
    }
    #[inline(always)]
    unsafe fn rcu_from(v: usize) -> Self {
        use core::mem::{size_of, transmute_copy};
        Self::rcu_assert();
        const USIZE_SIZE: usize = size_of::<usize>();
        match size_of::<Self>() {
            1 => transmute_copy(&(v as u8)),
            2 => transmute_copy(&(v as u16)),
            4 => transmute_copy(&(v as u32)),
            USIZE_SIZE => transmute_copy(&v),
            size => unreachable!("no support size: {}", size),
        }
    }
    #[must_use]
    #[inline(always)]
    unsafe fn rcu_transmute(self) -> (usize, unsafe fn(usize)) {
        (Self::rcu_into(self), Self::drop_fn())
    }
    #[inline]
    fn rcu_drop(self) {
        self::rcu_drop(self)
    }
    #[inline(always)]
    fn drop_fn() -> unsafe fn(usize) {
        |a| unsafe { drop(Self::rcu_from(a)) }
    }
}

impl<T: Sized + 'static> RcuCollect for Box<T> {}
impl<T: Sized + 'static> RcuCollect for Arc<T> {}
impl<T: Sized + 'static> RcuCollect for Weak<T> {}

/// rcu_write需要手动串行化
pub struct RcuWraper<T: RcuCollect>(T);

impl<T: RcuCollect> RcuWraper<T> {
    #[inline(always)]
    pub const fn new(value: T) -> Self {
        Self(value)
    }
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.0
    }
    /// 不要同时持有rcu_read的多个guard, 读取到的数据可能是不相同的!
    #[inline(always)]
    pub fn rcu_read(&self) -> impl Deref<Target = T> + '_ {
        self.0.rcu_read()
    }
    /// 需要手动加锁
    #[inline]
    pub unsafe fn rcu_write(&self, src: T) {
        self.0.rcu_write(src)
    }
}

pub struct LockedRcuWrapper<T: RcuCollect, S: MutexSupport>(SpinMutex<T, S>);

impl<T: RcuCollect, S: MutexSupport> LockedRcuWrapper<T, S> {
    #[inline(always)]
    pub const fn new(value: T) -> Self {
        Self(SpinMutex::new(value))
    }
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
    #[inline(always)]
    pub fn rcu_read(&self) -> impl Deref<Target = T> + '_ {
        unsafe { self.0.unsafe_get().rcu_read() }
    }
    #[inline]
    pub fn rcu_write(&self, src: T) {
        unsafe { self.0.lock().rcu_write(src) }
    }
    /// 此函数和rcu不可同时使用
    #[inline]
    pub unsafe fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        self.0.lock()
    }
}

static mut RCU_DROP_FN: Option<fn((usize, unsafe fn(usize)))> = None;
static RCU_DROP_PENDING: SpinMutex<Vec<(usize, unsafe fn(usize))>, Spin> =
    SpinMutex::new(Vec::new());

pub fn init(rcu_drop_fn: fn((usize, unsafe fn(usize)))) {
    unsafe {
        RCU_DROP_FN.replace(rcu_drop_fn);
        let v = core::mem::take(&mut *RCU_DROP_PENDING.lock());
        v.into_iter().for_each(rcu_drop_fn);
    }
}

#[inline]
pub fn rcu_drop<T: RcuCollect>(x: T) {
    match unsafe { RCU_DROP_FN } {
        Some(rcu_drop) => unsafe { rcu_drop(x.rcu_transmute()) },
        None => RCU_DROP_PENDING.lock().push(unsafe { x.rcu_transmute() }),
    }
}
