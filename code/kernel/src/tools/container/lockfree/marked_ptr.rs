use core::{
    marker::PhantomData,
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

/// MarkedRV39Ptr 高 25 位放置计数器
///
/// [63:39|38:0] [id:value]
///
/// 拥有25位ID, 循环周期 33554432
pub struct MarkedPtr<T>(usize, PhantomData<*mut T>);

impl<T> Clone for MarkedPtr<T> {
    fn clone(&self) -> Self {
        Self(self.0, PhantomData)
    }
}
impl<T> Copy for MarkedPtr<T> {}

impl<T> PartialEq for MarkedPtr<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

pub struct AtomicMarkedPtr<T>(AtomicUsize, PhantomData<*mut T>);

/// 定义 PtrID 的低39位都是 0
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtrID(usize);

impl PtrID {
    pub fn zero() -> Self {
        Self(0)
    }
    pub fn num(self) -> usize {
        self.0 >> 39
    }
    pub fn confuse(self) -> Self {
        Self((self.num() * 2381 + 523) << 39)
    }
}

const PTR_MASK: usize = (1 << 39) - 1;
const PTR_ID_BASE: usize = 1 << 39;

const INVALID: usize = 1; // container closed

impl<T> MarkedPtr<T> {
    pub fn new(id: PtrID, ptr: Option<NonNull<T>>) -> Self {
        let value: usize = unsafe { core::mem::transmute(ptr) };
        Self(id.0 | value & PTR_MASK, PhantomData)
    }
    pub fn new_invalid(id: PtrID) -> Self {
        Self::new(id, NonNull::new(INVALID as *mut _))
    }
    pub fn valid(self) -> Result<(), ()> {
        if ((self.0 << 25) as isize >> 25) as usize != INVALID {
            Ok(())
        } else {
            Err(())
        }
    }
    pub fn null(id: PtrID) -> Self {
        Self::new(id, None)
    }
    pub fn into_null(self) -> Self {
        Self::null(self.id())
    }
    fn from_usize(a: usize) -> Self {
        Self(a, PhantomData)
    }
    pub fn id(self) -> PtrID {
        PtrID(self.0 & !PTR_MASK)
    }
    pub fn get_ptr(self) -> Option<NonNull<T>> {
        // sign extend by 38th bit
        let ptr = (self.0 << 25) as isize >> 25;
        NonNull::new(ptr as *mut _)
    }
    pub fn get_mut(&self) -> Option<&mut T> {
        self.get_ptr().map(|a| unsafe { &mut *a.as_ptr() })
    }
    pub fn cast<V>(self) -> MarkedPtr<V> {
        unsafe { core::mem::transmute(self) }
    }
}

impl<T> AtomicMarkedPtr<T> {
    pub const fn null() -> Self {
        Self(AtomicUsize::new(0), PhantomData)
    }
    pub const fn invalid() -> Self {
        Self(AtomicUsize::new(usize::MAX), PhantomData)
    }
    pub fn new(ptr: MarkedPtr<T>) -> Self {
        Self(AtomicUsize::new(ptr.0), PhantomData)
    }
    pub fn confusion(&self) {
        self.0.fetch_xor(0x1a1a1a1a1a1a1a1a, Ordering::Release);
    }
    pub fn set_id_null(&mut self, id: PtrID) {
        self.0.store(id.0, Ordering::Relaxed);
    }
    pub fn load(&self) -> MarkedPtr<T> {
        MarkedPtr::from_usize(self.0.load(Ordering::Acquire))
    }
    pub fn compare_exchange(
        &self,
        current: MarkedPtr<T>,
        new: MarkedPtr<T>,
    ) -> Result<MarkedPtr<T>, MarkedPtr<T>> {
        match self.0.compare_exchange(
            current.0,
            new.0.wrapping_add(PTR_ID_BASE),
            Ordering::SeqCst,
            Ordering::Acquire,
        ) {
            Ok(x) => Ok(MarkedPtr::from_usize(x)),
            Err(x) => Err(MarkedPtr::from_usize(x)),
        }
    }
}
