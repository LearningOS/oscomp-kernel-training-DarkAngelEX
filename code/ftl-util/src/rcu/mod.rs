use core::{
    cell::SyncUnsafeCell,
    marker::PhantomData,
    mem::ManuallyDrop,
    ops::{Deref, DerefMut},
    ptr::NonNull,
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
    _mark: PhantomData<&'a T>,
    value: ManuallyDrop<T>,
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

pub struct RcuDrop(usize, unsafe fn(usize));
impl Drop for RcuDrop {
    fn drop(&mut self) {
        panic!("leaked RcuDrop");
    }
}
impl RcuDrop {
    /// # Safety
    /// 
    /// 只能由RCU控制器释放
    #[inline(always)]
    pub unsafe fn release(self) {
        self.1(self.0);
        core::mem::forget(self);
    }
}

/// RCU类型需要保证load和store操作都能使用一条指令执行, 保证读写的原子性
///
/// 因此RCU类型的大小不能超过usize, align必须和大小一致. 例如Box<dyn T>就是无法RCU的类型
///
/// 对于这些较大的无法RCU的类型, 序列锁是更理想的选择, 且读端也没有原子开销。
///
/// 如果类型不需要析构, 释放时不会加入释放队列以降低开销.
pub trait RcuCollect: Sized + 'static {
    /// 防止手抽把不该RCU的类型给RCU了
    #[inline(always)]
    fn rcu_assert() {
        use core::mem::{align_of, size_of};
        // 这几个判断将在编译时被计算
        assert_eq!(size_of::<Self>(), align_of::<Self>()); // size和align相等时可被一条指令执行
        assert!(size_of::<Self>() <= size_of::<usize>());
        assert!(size_of::<u32>() <= size_of::<usize>());
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
    /// # Safety
    /// 
    /// 用户需要保证此函数有锁
    #[inline]
    unsafe fn rcu_write(&self, src: Self) {
        Self::rcu_assert();
        atomic::fence(atomic::Ordering::Release);
        core::ptr::replace(self as *const _ as *mut Self, src).rcu_drop();
    }
    /// 使用原子替换方式修改, 此方式不需要额外的锁
    #[inline]
    fn rcu_write_atomic(&self, src: Self) {
        Self::rcu_assert();
        unsafe {
            use atomic::Ordering::Release;
            let new = rcu_into(src);
            macro_rules! atomic_swap_impl {
                ($at: ident, $ut: ty) => {{
                    use core::sync::atomic::$at;
                    core::mem::transmute::<_, &$at>(self).swap(new as $ut, Release) as usize
                }};
            }
            let old = match core::mem::size_of::<Self>() {
                1 => atomic_swap_impl!(AtomicU8, u8),
                2 => atomic_swap_impl!(AtomicU16, u16),
                4 => atomic_swap_impl!(AtomicU32, u32),
                8 => atomic_swap_impl!(AtomicU64, u64),
                _ => panic!(),
            };
            rcu_from::<Self>(old).rcu_drop();
        }
    }
    /// # Safety
    /// 
    /// 需要用管理器系统释放 `RcuDrop`
    #[must_use]
    #[inline(always)]
    unsafe fn rcu_transmute(self) -> RcuDrop {
        RcuDrop(rcu_into(self), rcu_drop_fn::<Self>())
    }
    #[inline]
    fn rcu_drop(self) {
        self::rcu_drop(self)
    }
}

#[must_use]
#[inline(always)]
unsafe fn rcu_into<T: RcuCollect>(this: T) -> usize {
    use core::mem::{size_of, transmute_copy};
    T::rcu_assert();
    const USIZE_SIZE: usize = size_of::<usize>();
    let v = match size_of::<T>() {
        1 => transmute_copy::<T, u8>(&this) as usize,
        2 => transmute_copy::<T, u16>(&this) as usize,
        4 => transmute_copy::<T, u32>(&this) as usize,
        USIZE_SIZE => transmute_copy::<T, usize>(&this),
        size => unreachable!("no support size: {}", size),
    };
    core::mem::forget(this);
    v
}

#[inline(always)]
unsafe fn rcu_from<T: RcuCollect>(v: usize) -> T {
    use core::mem::{size_of, transmute_copy};
    T::rcu_assert();
    const USIZE_SIZE: usize = size_of::<usize>();
    match size_of::<T>() {
        1 => transmute_copy(&(v as u8)),
        2 => transmute_copy(&(v as u16)),
        4 => transmute_copy(&(v as u32)),
        USIZE_SIZE => transmute_copy(&v),
        size => unreachable!("no support size: {}", size),
    }
}

#[inline(always)]
fn rcu_drop_fn<T: RcuCollect>() -> unsafe fn(usize) {
    |a| unsafe { drop(rcu_from::<T>(a)) }
}

impl<T: 'static> RcuCollect for *const T {}
impl<T: 'static> RcuCollect for *mut T {}
impl<T: 'static> RcuCollect for NonNull<T> {}
impl<T: 'static> RcuCollect for Box<T> {}
impl<T: 'static> RcuCollect for Arc<T> {}
impl<T: 'static> RcuCollect for Weak<T> {
    #[inline(always)]
    fn rcu_drop(self) {
        // 当Weak没有指向具体对象时不占用RCU资源
        if !self.ptr_eq(&Weak::new()) {
            self::rcu_drop(self)
        }
    }
}
/// 当值为None时不占用RCU资源
macro_rules! option_rcu_impl {
    ($T: ident, $name: ty) => {
        impl<$T: 'static> RcuCollect for $name {
            #[inline(always)]
            fn rcu_drop(self) {
                if let Some(p) = self {
                    self::rcu_drop(p);
                }
            }
        }
    };
}
option_rcu_impl!(T, Option<NonNull<T>>);
option_rcu_impl!(T, Option<Box<T>>);
option_rcu_impl!(T, Option<Arc<T>>);
option_rcu_impl!(T, Option<Weak<T>>);

/// rcu_write需要手动串行化
pub struct RcuWraper<T: RcuCollect>(SyncUnsafeCell<T>);

impl<T: RcuCollect> RcuWraper<T> {
    #[inline(always)]
    pub const fn new(value: T) -> Self {
        Self(SyncUnsafeCell::new(value))
    }
    #[inline(always)]
    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
    /// 不要同时持有rcu_read的多个guard, 读取到的数据可能是不相同的!
    #[inline(always)]
    pub fn rcu_read(&self) -> impl Deref<Target = T> + '_ {
        unsafe { &*self.0.get() }.rcu_read()
    }
    /// # Safety
    /// 
    /// 需要手动加锁
    #[inline]
    pub unsafe fn rcu_write(&self, src: T) {
        (&*self.0.get()).rcu_write(src)
    }
    pub fn rcu_write_atomic(&self, src: T) {
        unsafe { (&*self.0.get()).rcu_write_atomic(src) }
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
    /// # Safety
    /// 
    /// 此函数和rcu_read不可同时使用
    #[inline]
    pub unsafe fn lock(&self) -> impl DerefMut<Target = T> + '_ {
        self.0.lock()
    }
}

static mut RCU_DROP_FN: Option<fn(RcuDrop)> = None;
static RCU_DROP_PENDING: SpinMutex<Vec<RcuDrop>, Spin> = SpinMutex::new(Vec::new());

pub fn init(rcu_drop_fn: fn(RcuDrop)) {
    unsafe {
        RCU_DROP_FN.replace(rcu_drop_fn);
        let v = core::mem::take(&mut *RCU_DROP_PENDING.lock());
        v.into_iter().for_each(rcu_drop_fn);
    }
}

#[inline]
fn rcu_drop<T: RcuCollect>(x: T) {
    if !core::mem::needs_drop::<T>() {
        return;
    }
    match unsafe { RCU_DROP_FN } {
        Some(rcu_drop) => unsafe { rcu_drop(x.rcu_transmute()) },
        None => RCU_DROP_PENDING.lock().push(unsafe { x.rcu_transmute() }),
    }
}
