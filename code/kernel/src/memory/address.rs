use core::{
    convert::TryFrom,
    fmt::{self, Debug, Formatter},
    marker::PhantomData,
    ops::{Add, AddAssign, Sub, SubAssign},
};

use crate::{
    config::{
        DIRECT_MAP_OFFSET, DIRECT_MAP_SIZE, PAGE_SIZE, PAGE_SIZE_BITS, USER_END, USER_HEAP_BEGIN,
    },
    impl_usize_from,
    syscall::{SysError, UniqueSysError},
    tools,
};

use super::{
    page_table::PageTableEntry,
    user_ptr::{Policy, UserPtr},
};

// const PA_WIDTH_SV39: usize = 56;
// const VA_WIDTH_SV39: usize = 39;
// const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;
// const VPN_WIDTH_SV39: usize = VA_WIDTH_SV39 - PAGE_SIZE_BITS;

#[repr(C)]
pub struct VirAddr<T>(usize, PhantomData<*const T>);

///
/// PhyAddr can't deref, need to into PhyAddrRef
#[repr(C)]
pub struct PhyAddr<T>(usize, PhantomData<*const T>);

/// direct mapping to physical address
///
/// same as PhyAddr + PHYSICAL_MEMORY_OFFSET
#[repr(C)]
pub struct PhyAddrRef<T>(usize, PhantomData<*const T>);

#[repr(C)]
/// only valid in user space
pub struct UserAddr<T>(usize, PhantomData<*const T>);

/// assert self & 0xfff = 0
#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirAddr4K(usize);

/// assert self & 0xfff = 0
#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddr4K(usize);

/// assert self & 0xfff = 0
#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddrRef4K(usize);

/// assert self & 0xfff = 0
#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct UserAddr4K(usize);

#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PageCount(pub usize);

macro_rules! send_sync {
    ($T: ident, $name: ty) => {
        unsafe impl<$T> Send for $name {}
        unsafe impl<$T> Sync for $name {}
        impl<$T> Copy for $name {}
        impl<$T> Clone for $name {
            #[inline(always)]
            fn clone(&self) -> Self {
                Self::new(self.0)
            }
        }
        impl<$T> Eq for $name {}
        impl<$T> PartialEq for $name {
            #[inline(always)]
            fn eq(&self, r: &Self) -> bool {
                self.0.eq(&r.0)
            }
        }
        impl<$T> Ord for $name {
            #[inline(always)]
            fn cmp(&self, r: &Self) -> core::cmp::Ordering {
                self.0.cmp(&r.0)
            }
        }
        impl<$T> PartialOrd for $name {
            #[inline(always)]
            fn partial_cmp(&self, r: &Self) -> Option<core::cmp::Ordering> {
                self.0.partial_cmp(&r.0)
            }
        }
    };
}
send_sync!(T, VirAddr<T>);
send_sync!(T, PhyAddr<T>);
send_sync!(T, UserAddr<T>);
send_sync!(T, PhyAddrRef<T>);

#[derive(Debug)]
pub struct OutOfUserRange;

impl From<OutOfUserRange> for SysError {
    #[inline(always)]
    fn from(_e: OutOfUserRange) -> Self {
        SysError::EFAULT
    }
}
impl From<OutOfUserRange> for UniqueSysError<{ SysError::EFAULT as isize }> {
    #[inline(always)]
    fn from(_: OutOfUserRange) -> Self {
        Self
    }
}

/// Debugging

impl<T> Debug for VirAddr<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl<T> Debug for PhyAddr<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl<T> Debug for PhyAddrRef<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA ref:{:#x}", self.0))
    }
}
impl<T> Debug for UserAddr<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("UA:{:#x}", self.0))
    }
}
impl Debug for PhyAddr4K {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA 4K:{:#x}", self.0))
    }
}
impl Debug for VirAddr4K {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA 4K:{:#x}", self.0))
    }
}
impl Debug for PhyAddrRef4K {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA ref 4K:{:#x}", self.0))
    }
}
impl Debug for UserAddr4K {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("UA 4K:{:#x}", self.0))
    }
}

impl Debug for PageCount {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PC:{:#x}", self.0))
    }
}

/// T: {PhysAddr, VirtAddr, PhysPageNum, VirtPageNum}
/// T -> usize: T.0
/// usize -> T: usize.into()

macro_rules! impl_from_usize {
    ($T: ident, $name: ty, $v: ident, $body: stmt, $check_fn: stmt) => {
        impl<$T> const From<usize> for $name {
            #[inline(always)]
            fn from($v: usize) -> Self {
                $check_fn
                $body
            }
        }
    };
}

// impl_from_usize!(VirAddr, v, Self(v & ((1 << VA_WIDTH_SV39) - 1)));
// impl_from_usize!(PhyAddr, v, Self(v & ((1 << PA_WIDTH_SV39) - 1)));
impl_from_usize!(T, VirAddr<T>, v, Self::new(v), ());
impl_from_usize!(
    T,
    PhyAddr<T>,
    v,
    Self::new(v),
    debug_assert!(v < DIRECT_MAP_SIZE)
);
impl_from_usize!(
    T,
    PhyAddrRef<T>,
    v,
    Self::new(v),
    debug_assert!(v - DIRECT_MAP_OFFSET < DIRECT_MAP_SIZE)
);
impl_from_usize!(T, UserAddr<T>, v, Self::new(v), ());

macro_rules! impl_usize_from_t {
    ($T: ident, $name: ty, $v: ident, $body: stmt) => {
        impl<$T> From<$name> for usize {
            #[inline(always)]
            fn from($v: $name) -> Self {
                $body
            }
        }
        impl<$T> $name {
            #[inline(always)]
            pub const fn into_usize(self) -> usize {
                let $v = self;
                $body
            }
        }
    };
}

impl_usize_from_t!(T, VirAddr<T>, v, v.0);
impl_usize_from_t!(T, PhyAddr<T>, v, v.0);
impl_usize_from_t!(T, PhyAddrRef<T>, v, v.0);
impl_usize_from_t!(T, UserAddr<T>, v, v.0);
impl_usize_from!(VirAddr4K, v, v.0);
impl_usize_from!(PhyAddr4K, v, v.0);
impl_usize_from!(PhyAddrRef4K, v, v.0);
impl_usize_from!(UserAddr4K, v, v.0);
impl_usize_from!(PageCount, v, v.0);

macro_rules! impl_addr_4K_common {
    ($T: ident, $name: ty, $x4K_name: ident) => {
        impl<$T> $name {
            #[inline(always)]
            const fn new(v: usize) -> Self {
                Self(v, PhantomData)
            }
            #[inline(always)]
            pub const fn is_null(self) -> bool {
                self.0 == 0
            }
            #[inline(always)]
            pub const fn floor(self) -> $x4K_name {
                $x4K_name(self.0 & !(PAGE_SIZE - 1))
            }
            #[inline(always)]
            pub const fn ceil(self) -> $x4K_name {
                $x4K_name((self.0 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
            }
            #[inline(always)]
            pub const fn page_offset(self) -> usize {
                self.0 & (PAGE_SIZE - 1)
            }
            #[inline(always)]
            pub const fn aligned(self) -> bool {
                self.page_offset() == 0
            }
        }
        impl $x4K_name {
            #[inline(always)]
            const fn new(v: usize) -> Self {
                Self(v)
            }
            /// return self - other
            #[inline(always)]
            pub const fn offset_to(self, other: Self) -> PageCount {
                debug_assert!(self.0 >= other.0);
                PageCount((self.0 - other.0) / PAGE_SIZE)
            }
        }
        impl<$T> From<$name> for $x4K_name {
            #[inline(always)]
            fn from(v: $name) -> Self {
                v.floor()
            }
        }
        impl<$T> From<$x4K_name> for $name {
            #[inline(always)]
            fn from(v: $x4K_name) -> Self {
                Self::new(v.0)
            }
        }
    };
}

impl_addr_4K_common!(T, VirAddr<T>, VirAddr4K);
impl_addr_4K_common!(T, PhyAddr<T>, PhyAddr4K);
impl_addr_4K_common!(T, PhyAddrRef<T>, PhyAddrRef4K);
impl_addr_4K_common!(T, UserAddr<T>, UserAddr4K);

macro_rules! impl_phy_ref_translate {
    ($phy_name: ty, $phy_ref_name: ty) => {
        impl From<$phy_name> for $phy_ref_name {
            #[inline(always)]
            fn from(v: $phy_name) -> Self {
                Self::new(usize::from(v) + DIRECT_MAP_OFFSET)
            }
        }
        impl From<$phy_ref_name> for $phy_name {
            #[inline(always)]
            fn from(v: $phy_ref_name) -> Self {
                Self::new(usize::from(v) - DIRECT_MAP_OFFSET)
            }
        }
    };
    ($T: ident, $phy_name: ty, $phy_ref_name: ty) => {
        impl<$T> From<$phy_name> for $phy_ref_name {
            #[inline(always)]
            fn from(v: $phy_name) -> Self {
                Self::new(usize::from(v) + DIRECT_MAP_OFFSET)
            }
        }
        impl<$T> From<$phy_ref_name> for $phy_name {
            #[inline(always)]
            fn from(v: $phy_ref_name) -> Self {
                Self::new(usize::from(v) - DIRECT_MAP_OFFSET)
            }
        }
    };
}

impl_phy_ref_translate!(T, PhyAddr<T>, PhyAddrRef<T>);
impl_phy_ref_translate!(PhyAddr4K, PhyAddrRef4K);

impl<T> From<UserAddr<T>> for VirAddr<T> {
    #[inline(always)]
    fn from(ua: UserAddr<T>) -> Self {
        Self::new(ua.into())
    }
}

impl<T> TryFrom<*const T> for UserAddr<T> {
    type Error = OutOfUserRange;
    #[inline(always)]
    fn try_from(value: *const T) -> Result<Self, Self::Error> {
        let r = Self::new(value as usize);
        match r.valid() {
            Ok(_) => Ok(r),
            Err(_) => Err(OutOfUserRange),
        }
    }
}
impl<T> TryFrom<*mut T> for UserAddr<T> {
    type Error = OutOfUserRange;
    #[inline(always)]
    fn try_from(value: *mut T) -> Result<Self, Self::Error> {
        let r = Self::new(value as usize);
        match r.valid() {
            Ok(_) => Ok(r),
            Err(_) => Err(OutOfUserRange),
        }
    }
}
impl<T: Clone + Copy + 'static, P: Policy> TryFrom<UserPtr<T, P>> for UserAddr<T> {
    type Error = OutOfUserRange;
    #[inline(always)]
    fn try_from(value: UserPtr<T, P>) -> Result<Self, Self::Error> {
        let r = Self(value.as_usize(), PhantomData);
        match r.valid() {
            Ok(_) => Ok(r),
            Err(_) => Err(OutOfUserRange),
        }
    }
}

macro_rules! add_sub_impl {
    ($name_4k: ty) => {
        impl $name_4k {
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            #[inline(always)]
            pub const fn add_one_page(self) -> Self {
                Self::new(self.0 + PAGE_SIZE)
            }
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            #[inline(always)]
            pub const fn sub_one_page(self) -> Self {
                Self::new(self.0 - PAGE_SIZE)
            }
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            #[inline(always)]
            pub const fn add_page(self, n: PageCount) -> Self {
                Self::new(self.0 + n.byte_space())
            }
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            #[inline(always)]
            pub const fn sub_page(self, n: PageCount) -> Self {
                Self::new(self.0 - n.byte_space())
            }
            #[inline(always)]
            pub fn add_page_assign(&mut self, n: PageCount) {
                self.0 += n.byte_space()
            }
            #[inline(always)]
            pub fn sub_page_assign(&mut self, n: PageCount) {
                self.0 -= n.byte_space()
            }
        }
    };
    ($T: ident, $name_4k: ty) => {
        impl<$T> $name_4k {
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            pub const fn add_one_page(self) -> Self {
                Self::new(self.0 + PAGE_SIZE)
            }
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            pub const fn sub_one_page(self) -> Self {
                Self::new(self.0 - PAGE_SIZE)
            }
            #[must_use = "the answer is in the return value!"]
            /// the answer is in the return value!
            pub const fn add_page(self, n: PageCount) -> Self {
                Self::new(self.0 + n.byte_space())
            }
            #[must_use = "the answer is in the return value!"]
            /// the answer is in the return value!
            pub const fn sub_page(self, n: PageCount) -> Self {
                Self::new(self.0 - n.byte_space())
            }
            #[inline(always)]
            pub fn add_page_assign(&mut self, n: PageCount) {
                self.0 += n.byte_space()
            }
            #[inline(always)]
            pub fn sub_page_assign(&mut self, n: PageCount) {
                self.0 -= n.byte_space()
            }
        }
    };
}

add_sub_impl!(T, VirAddr<T>);
add_sub_impl!(T, PhyAddr<T>);
add_sub_impl!(T, PhyAddrRef<T>);
add_sub_impl!(VirAddr4K);
add_sub_impl!(PhyAddr4K);
add_sub_impl!(PhyAddrRef4K);

impl<T> UserAddr<T> {
    #[inline(always)]
    pub const fn null() -> Self {
        unsafe { Self::from_usize(0) }
    }
    #[inline(always)]
    pub const fn is_4k_align(self) -> bool {
        (self.into_usize() % PAGE_SIZE) == 0
    }
    #[inline(always)]
    pub const fn valid(self) -> Result<(), ()> {
        tools::bool_result(self.0 <= USER_END)
    }
    #[inline(always)]
    pub const unsafe fn from_usize(addr: usize) -> Self {
        Self::new(addr)
    }
    #[inline(always)]
    pub fn get_mut(self) -> &'static mut T {
        unsafe { &mut *(self.0 as *mut T) }
    }
    #[inline(always)]
    pub fn add_assign(&mut self, num: usize) {
        self.0 += num
    }
    #[inline(always)]
    pub fn sub_assign(&mut self, num: usize) {
        self.0 -= num
    }
    #[inline(always)]
    pub unsafe fn as_ptr(self) -> &'static T {
        &*(self.0 as *const T)
    }
    #[inline(always)]
    pub unsafe fn as_ptr_mut(self) -> &'static mut T {
        &mut *(self.0 as *mut T)
    }
}
impl<T> VirAddr<T> {
    #[inline(always)]
    pub fn as_ptr(self) -> *const T {
        self.into_usize() as *const T
    }
    #[inline(always)]
    pub fn as_ptr_mut(self) -> *mut T {
        self.into_usize() as *mut T
    }
    #[inline(always)]
    pub unsafe fn as_ref(self) -> &'static T {
        &*self.as_ptr()
    }
    #[inline(always)]
    pub unsafe fn as_mut(self) -> &'static mut T {
        &mut *self.as_ptr_mut()
    }
}
impl VirAddr4K {
    #[inline(always)]
    pub const fn indexes(self) -> [usize; 3] {
        let v = self.vpn();
        const fn f(a: usize) -> usize {
            a & 0x1ff
        }
        [f(v >> 18), f(v >> 9), f(v)]
    }
    #[inline(always)]
    pub const fn vpn(self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    #[inline(always)]
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
}
impl<T> PhyAddr<T> {
    #[inline(always)]
    pub const fn from_usize(n: usize) -> Self {
        Self::new(n)
    }
    #[inline(always)]
    pub fn into_ref(self) -> PhyAddrRef<T> {
        PhyAddrRef::from(self)
    }
}
impl<T> PhyAddrRef<T> {
    #[inline(always)]
    pub unsafe fn get(self) -> &'static T {
        &*(self.0 as *const T)
    }
    #[inline(always)]
    pub unsafe fn get_mut(self) -> &'static mut T {
        &mut *(self.0 as *mut T)
    }
}
impl PhyAddr4K {
    /// physical page number
    #[inline(always)]
    pub const fn ppn(self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    #[inline(always)]
    pub const fn from_satp(satp: usize) -> Self {
        // let ppn = satp & (1usize << 44) - 1;
        let ppn = satp & ((1usize << 38) - 1);
        Self(ppn << 12)
    }
    /// assume n % PAGE_SIZE
    #[inline(always)]
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
    #[inline(always)]
    pub fn into_ref(self) -> PhyAddrRef4K {
        PhyAddrRef4K::from(self)
    }
}

impl PhyAddrRef4K {
    #[inline(always)]
    pub fn as_pte_array(self) -> &'static [PageTableEntry; 512] {
        self.as_mut()
    }
    #[inline(always)]
    pub fn as_pte_array_mut(self) -> &'static mut [PageTableEntry; 512] {
        self.as_mut()
    }
    #[inline(always)]
    pub fn as_bytes_array(self) -> &'static [u8; 4096] {
        self.as_mut()
    }
    #[inline(always)]
    pub fn as_bytes_array_mut(self) -> &'static mut [u8; 4096] {
        self.as_mut()
    }
    #[inline(always)]
    pub fn as_usize_array(self) -> &'static [usize; 512] {
        self.as_mut()
    }
    #[inline(always)]
    pub fn as_usize_array_mut(self) -> &'static mut [usize; 512] {
        self.as_mut()
    }
    #[inline(always)]
    pub fn as_mut<T>(self) -> &'static mut T {
        let pa: PhyAddrRef<T> = self.into();
        unsafe { &mut *(pa.0 as *mut T) }
    }
    #[inline(always)]
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
}

impl UserAddr4K {
    #[inline(always)]
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
    #[inline(always)]
    pub const fn from_usize_check(n: usize) -> Self {
        assert!(n % PAGE_SIZE == 0 && n <= USER_END);
        Self(n)
    }
    #[inline(always)]
    pub const fn valid(self) -> Result<(), ()> {
        tools::bool_result(self.0 <= USER_END)
    }
    #[inline(always)]
    pub const fn heap_offset(n: PageCount) -> Self {
        Self(USER_HEAP_BEGIN + n.byte_space())
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    #[inline(always)]
    pub const fn add_one_page(self) -> Self {
        Self(self.0 + PAGE_SIZE)
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    #[inline(always)]
    pub const fn sub_one_page(self) -> Self {
        Self(self.0 - PAGE_SIZE)
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    #[inline(always)]
    pub const fn add_page(self, n: PageCount) -> Self {
        Self(self.0 + n.byte_space())
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    #[inline(always)]
    pub const fn add_page_checked(self, n: PageCount) -> Result<Self, OutOfUserRange> {
        let v = Self(self.0 + n.byte_space());
        if v.valid().is_err() {
            return Err(OutOfUserRange);
        }
        Ok(v)
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    #[inline(always)]
    pub const fn sub_page(self, n: PageCount) -> Self {
        Self(self.0 - n.byte_space())
    }
    #[inline(always)]
    pub fn add_page_assign(&mut self, n: PageCount) {
        self.0 += n.byte_space()
    }
    #[inline(always)]
    pub fn sub_page_assign(&mut self, n: PageCount) {
        self.0 -= n.byte_space()
    }
    #[inline(always)]
    pub const fn vpn(self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    #[inline(always)]
    pub const fn indexes(self) -> [usize; 3] {
        let v = self.vpn();
        const fn f(a: usize) -> usize {
            a & 0x1ff
        }
        [f(v >> 18), f(v >> 9), f(v)]
    }
    #[inline(always)]
    pub const fn from_indexes([a, b, c]: [usize; 3]) -> Self {
        Self((a << 30) | (b << 21) | (c << 12))
    }
    #[inline(always)]
    pub const fn null() -> Self {
        unsafe { Self::from_usize(0) }
    }
    #[inline(always)]
    pub const fn user_max() -> Self {
        unsafe { Self::from_usize(USER_END) }
    }
}

impl PageCount {
    #[inline(always)]
    pub const fn from_usize(v: usize) -> Self {
        Self(v)
    }
    #[inline(always)]
    pub const fn byte_space(self) -> usize {
        self.0 * PAGE_SIZE
    }
    #[inline(always)]
    pub const fn page_floor(a: usize) -> Self {
        Self(a / PAGE_SIZE)
    }
    #[inline(always)]
    pub const fn page_ceil(a: usize) -> Self {
        Self((a + PAGE_SIZE - 1) / PAGE_SIZE)
    }
}

impl Add for PageCount {
    type Output = PageCount;
    #[inline(always)]
    fn add(self, rhs: Self) -> Self::Output {
        Self::Output::from_usize(self.0 + rhs.0)
    }
}
impl AddAssign for PageCount {
    #[inline(always)]
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0
    }
}
impl Sub for PageCount {
    type Output = PageCount;
    #[inline(always)]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::Output::from_usize(self.0 - rhs.0)
    }
}
impl SubAssign for PageCount {
    #[inline(always)]
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0
    }
}

pub trait StepByOne {
    fn step(&mut self);
}

macro_rules! impl_step_by_one {
    ($name: ident) => {
        impl StepByOne for $name {
            #[inline(always)]
            fn step(&mut self) {
                self.0 += PAGE_SIZE;
            }
        }
    };
}

impl_step_by_one!(VirAddr4K);
impl_step_by_one!(PhyAddr4K);
impl_step_by_one!(PhyAddrRef4K);
impl_step_by_one!(UserAddr4K);
