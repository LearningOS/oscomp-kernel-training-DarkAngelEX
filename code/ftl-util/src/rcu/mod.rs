use alloc::{
    boxed::Box,
    rc,
    sync::{Arc, Weak},
};

pub mod manager;

/// Rcu类型宽度不能超过usize, align必须和size一致
pub trait RcuCollect: Sized + 'static {
    #[inline(always)]
    unsafe fn rcu_into(self) -> usize {
        use core::mem::{align_of, size_of, transmute_copy};
        assert_eq!(size_of::<Self>(), align_of::<Self>());
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
        use core::mem::{align_of, size_of, transmute_copy};
        assert_eq!(size_of::<Self>(), align_of::<Self>());
        const USIZE_SIZE: usize = size_of::<usize>();
        match size_of::<Self>() {
            1 => transmute_copy(&(v as u8)),
            2 => transmute_copy(&(v as u16)),
            4 => transmute_copy(&(v as u32)),
            USIZE_SIZE => transmute_copy(&v),
            size => unreachable!("no support size: {}", size),
        }
    }
    #[inline(always)]
    unsafe fn rcu_transmute(self) -> (usize, unsafe fn(usize)) {
        (Self::rcu_into(self), Self::drop_fn())
    }
    #[inline(always)]
    fn rcu_drop(self) {
        rcu_drop(self)
    }
    #[inline(always)]
    fn drop_fn() -> unsafe fn(usize) {
        |a| unsafe { drop(Self::rcu_from(a)) }
    }
}

impl<T: Sized + 'static> RcuCollect for Box<T> {}
impl<T: Sized + 'static> RcuCollect for Arc<T> {}
impl<T: Sized + 'static> RcuCollect for Weak<T> {}
impl<T: Sized + 'static> RcuCollect for rc::Rc<T> {}
impl<T: Sized + 'static> RcuCollect for rc::Weak<T> {}

static mut RCU_DROP_FN: Option<fn((usize, unsafe fn(usize)))> = None;

pub fn init(rcu_drop_fn: fn((usize, unsafe fn(usize)))) {
    unsafe {
        RCU_DROP_FN.replace(rcu_drop_fn);
    }
}

#[inline(always)]
pub fn rcu_drop<T: RcuCollect>(x: T) {
    match unsafe { RCU_DROP_FN } {
        Some(rcu_drop) => unsafe { rcu_drop(x.rcu_transmute()) },
        #[cfg(not(debug_assertions))]
        None => core::hint::unreachable_unchecked(),
        #[cfg(debug_assertions)]
        None => unimplemented!(),
    }
}
