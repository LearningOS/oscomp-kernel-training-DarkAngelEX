use core::{
    convert::TryFrom,
    fmt::{self, Debug, Formatter},
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

use super::{page_table::PageTableEntry, user_ptr::{UserPtr, Policy}};

// const PA_WIDTH_SV39: usize = 56;
// const VA_WIDTH_SV39: usize = 39;
// const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;
// const VPN_WIDTH_SV39: usize = VA_WIDTH_SV39 - PAGE_SIZE_BITS;

#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirAddr(usize);

///
/// PhyAddr can't deref, need to into PhyAddrRef
#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddr(usize);

/// direct mapping to physical address
///
/// same as PhyAddr + PHYSICAL_MEMORY_OFFSET
#[repr(C)]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddrRef(usize);

#[repr(C)]
/// only valid in user space
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct UserAddr(usize);

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
pub struct PageCount(usize);

#[derive(Debug)]
pub struct OutOfUserRange;

impl From<OutOfUserRange> for SysError {
    fn from(_e: OutOfUserRange) -> Self {
        SysError::EFAULT
    }
}
impl From<OutOfUserRange> for UniqueSysError<{ SysError::EFAULT as isize }> {
    fn from(_: OutOfUserRange) -> Self {
        Self
    }
}

/// Debugging

impl Debug for VirAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl Debug for PhyAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl Debug for PhyAddrRef {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA ref:{:#x}", self.0))
    }
}
impl Debug for UserAddr {
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
    ($name: ident, $v: ident, $body: stmt, $check_fn: stmt) => {
        impl From<usize> for $name {
            fn from($v: usize) -> Self {
                $check_fn
                $body
            }
        }
    };
}

// impl_from_usize!(VirAddr, v, Self(v & ((1 << VA_WIDTH_SV39) - 1)));
// impl_from_usize!(PhyAddr, v, Self(v & ((1 << PA_WIDTH_SV39) - 1)));
impl_from_usize!(VirAddr, v, Self(v), ());
impl_from_usize!(
    PhyAddr,
    v,
    Self(v),
    debug_check!(v < DIRECT_MAP_SIZE, "into PhyAddrRef error: {}", v)
);
impl_from_usize!(
    PhyAddrRef,
    v,
    Self(v),
    debug_check!(
        v - DIRECT_MAP_OFFSET < DIRECT_MAP_SIZE,
        "into PhyAddr error: {:x}",
        v
    )
);
impl_from_usize!(UserAddr, v, Self(v), ());

impl_usize_from!(VirAddr, v, v.0);
impl_usize_from!(PhyAddr, v, v.0);
impl_usize_from!(PhyAddrRef, v, v.0);
impl_usize_from!(UserAddr, v, v.0);
impl_usize_from!(VirAddr4K, v, v.0);
impl_usize_from!(PhyAddr4K, v, v.0);
impl_usize_from!(PhyAddrRef4K, v, v.0);
impl_usize_from!(UserAddr4K, v, v.0);
impl_usize_from!(PageCount, v, v.0);

macro_rules! impl_addr_4K_common {
    ($name: ident, $x4K_name: ident) => {
        impl $name {
            pub const fn floor(self) -> $x4K_name {
                $x4K_name(self.0 & !(PAGE_SIZE - 1))
            }
            pub const fn ceil(self) -> $x4K_name {
                $x4K_name((self.0 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
            }
            pub const fn page_offset(self) -> usize {
                self.0 & (PAGE_SIZE - 1)
            }
            pub const fn aligned(self) -> bool {
                self.page_offset() == 0
            }
        }
        impl From<$name> for $x4K_name {
            fn from(v: $name) -> Self {
                v.floor()
            }
        }
        impl From<$x4K_name> for $name {
            fn from(v: $x4K_name) -> Self {
                Self(v.0)
            }
        }
    };
}

impl_addr_4K_common!(VirAddr, VirAddr4K);
impl_addr_4K_common!(PhyAddr, PhyAddr4K);
impl_addr_4K_common!(PhyAddrRef, PhyAddrRef4K);
impl_addr_4K_common!(UserAddr, UserAddr4K);

macro_rules! impl_phy_ref_translate {
    ($phy_name: ident, $phy_ref_name: ident) => {
        impl From<$phy_name> for $phy_ref_name {
            fn from(v: $phy_name) -> Self {
                Self(usize::from(v) + DIRECT_MAP_OFFSET)
            }
        }
        impl From<$phy_ref_name> for $phy_name {
            fn from(v: $phy_ref_name) -> Self {
                Self(usize::from(v) - DIRECT_MAP_OFFSET)
            }
        }
    };
}

impl_phy_ref_translate!(PhyAddr, PhyAddrRef);
impl_phy_ref_translate!(PhyAddr4K, PhyAddrRef4K);

impl From<UserAddr> for VirAddr {
    fn from(ua: UserAddr) -> Self {
        Self(ua.into())
    }
}

impl<T> TryFrom<*const T> for UserAddr {
    type Error = OutOfUserRange;

    fn try_from(value: *const T) -> Result<Self, Self::Error> {
        let r = Self(value as usize);
        match r.valid() {
            Ok(_) => Ok(r),
            Err(_) => Err(OutOfUserRange),
        }
    }
}
impl<T> TryFrom<*mut T> for UserAddr {
    type Error = OutOfUserRange;

    fn try_from(value: *mut T) -> Result<Self, Self::Error> {
        let r = Self(value as usize);
        match r.valid() {
            Ok(_) => Ok(r),
            Err(_) => Err(OutOfUserRange),
        }
    }
}
impl<T: Clone + Copy + 'static, P: Policy> TryFrom<UserPtr<T, P>> for UserAddr {
    type Error = OutOfUserRange;

    fn try_from(value: UserPtr<T, P>) -> Result<Self, Self::Error> {
        let r = Self(value.as_usize());
        match r.valid() {
            Ok(_) => Ok(r),
            Err(_) => Err(OutOfUserRange),
        }
    }
}

macro_rules! add_sub_impl {
    ($name_4k: ident) => {
        impl $name_4k {
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            pub const fn add_one_page(self) -> Self {
                Self(self.0 + PAGE_SIZE)
            }
            /// the answer is in the return value!
            #[must_use = "the answer is in the return value!"]
            pub const fn sub_one_page(self) -> Self {
                Self(self.0 - PAGE_SIZE)
            }
            #[must_use = "the answer is in the return value!"]
            /// the answer is in the return value!
            pub const fn add_page(self, n: PageCount) -> Self {
                Self(self.0 + n.byte_space())
            }
            #[must_use = "the answer is in the return value!"]
            /// the answer is in the return value!
            pub const fn sub_page(self, n: PageCount) -> Self {
                Self(self.0 - n.byte_space())
            }
            pub fn add_page_assign(&mut self, n: PageCount) {
                self.0 += n.byte_space()
            }
            pub fn sub_page_assign(&mut self, n: PageCount) {
                self.0 -= n.byte_space()
            }
        }
    };
}

add_sub_impl!(VirAddr);
add_sub_impl!(PhyAddr);
add_sub_impl!(PhyAddrRef);
add_sub_impl!(VirAddr4K);
add_sub_impl!(PhyAddr4K);
add_sub_impl!(PhyAddrRef4K);

impl UserAddr {
    pub const fn is_4k_align(self) -> bool {
        (self.into_usize() % PAGE_SIZE) == 0
    }
    pub const fn valid(self) -> Result<(), ()> {
        tools::bool_result(self.0 <= USER_END)
    }
    pub const unsafe fn from_usize(addr: usize) -> Self {
        Self(addr)
    }
    pub fn get_mut<T>(self) -> &'static mut T {
        unsafe { &mut *(self.0 as *mut T) }
    }
    pub fn add_assign(&mut self, num: usize) {
        self.0 += num
    }
    pub unsafe fn as_ptr<T>(self) -> &'static T {
        &*(self.0 as *const T)
    }
    pub unsafe fn as_ptr_mut<T>(self) -> &'static mut T {
        &mut *(self.0 as *mut T)
    }
}
impl VirAddr {
    pub fn as_ptr<T>(self) -> *const T {
        self.into_usize() as *const T
    }
    pub fn as_ptr_mut<T>(self) -> *mut T {
        self.into_usize() as *mut T
    }
    pub unsafe fn as_ref<T>(self) -> &'static T {
        &*self.as_ptr()
    }
    pub unsafe fn as_mut<T>(self) -> &'static mut T {
        &mut *self.as_ptr_mut()
    }
}
impl VirAddr4K {
    pub const fn indexes(self) -> [usize; 3] {
        let v = self.vpn();
        const fn f(a: usize) -> usize {
            a & 0x1ff
        }
        [f(v >> 18), f(v >> 9), f(v)]
    }
    pub const fn vpn(self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
}
impl PhyAddr {
    pub const fn from_usize(n: usize) -> Self {
        Self(n)
    }
    pub fn into_ref(self) -> PhyAddrRef {
        PhyAddrRef::from(self)
    }
}
impl PhyAddr4K {
    /// physical page number
    pub const fn ppn(self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    pub const fn from_satp(satp: usize) -> Self {
        // let ppn = satp & (1usize << 44) - 1;
        let ppn = satp & ((1usize << 38) - 1);
        Self(ppn << 12)
    }
    /// assume n % PAGE_SIZE
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
    pub fn into_ref(self) -> PhyAddrRef4K {
        PhyAddrRef4K::from(self)
    }
}

impl PhyAddrRef4K {
    pub fn as_pte_array(self) -> &'static [PageTableEntry; 512] {
        self.as_mut()
    }
    pub fn as_pte_array_mut(self) -> &'static mut [PageTableEntry; 512] {
        self.as_mut()
    }
    pub fn as_bytes_array(self) -> &'static [u8; 4096] {
        self.as_mut()
    }
    pub fn as_bytes_array_mut(self) -> &'static mut [u8; 4096] {
        self.as_mut()
    }
    pub fn as_usize_array(self) -> &'static [usize; 512] {
        self.as_mut()
    }
    pub fn as_usize_array_mut(self) -> &'static mut [usize; 512] {
        self.as_mut()
    }
    pub fn as_mut<T>(self) -> &'static mut T {
        let pa: PhyAddrRef = self.into();
        unsafe { &mut *(pa.0 as *mut T) }
    }
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
}

impl UserAddr4K {
    pub const unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
    pub const fn from_usize_check(n: usize) -> Self {
        assert!(n % PAGE_SIZE == 0 && n <= USER_END);
        Self(n)
    }
    pub const fn heap_offset(n: PageCount) -> Self {
        Self(USER_HEAP_BEGIN + n.byte_space())
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    pub const fn add_one_page(self) -> Self {
        Self(self.0 + PAGE_SIZE)
    }
    /// the answer is in the return value!
    #[must_use = "the answer is in the return value!"]
    pub const fn sub_one_page(self) -> Self {
        Self(self.0 - PAGE_SIZE)
    }
    #[must_use = "the answer is in the return value!"]
    /// the answer is in the return value!
    pub const fn add_page(self, n: PageCount) -> Self {
        Self(self.0 + n.byte_space())
    }
    #[must_use = "the answer is in the return value!"]
    /// the answer is in the return value!
    pub const fn sub_page(self, n: PageCount) -> Self {
        Self(self.0 - n.byte_space())
    }
    pub fn add_page_assign(&mut self, n: PageCount) {
        self.0 += n.byte_space()
    }

    pub fn sub_page_assign(&mut self, n: PageCount) {
        self.0 -= n.byte_space()
    }
    pub const fn vpn(self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    pub const fn indexes(self) -> [usize; 3] {
        let v = self.vpn();
        const fn f(a: usize) -> usize {
            a & 0x1ff
        }
        [f(v >> 18), f(v >> 9), f(v)]
    }
    pub const fn null() -> Self {
        unsafe { Self::from_usize(0) }
    }
    pub const fn user_max() -> Self {
        unsafe { Self::from_usize(USER_END) }
    }
}

impl PageCount {
    pub const fn from_usize(v: usize) -> Self {
        Self(v)
    }
    pub const fn byte_space(self) -> usize {
        self.0 * PAGE_SIZE
    }
}

impl Add for PageCount {
    type Output = PageCount;

    fn add(self, rhs: Self) -> Self::Output {
        Self::Output::from_usize(self.0 + rhs.0)
    }
}
impl AddAssign for PageCount {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0
    }
}
impl Sub for PageCount {
    type Output = PageCount;

    fn sub(self, rhs: Self) -> Self::Output {
        Self::Output::from_usize(self.0 - rhs.0)
    }
}
impl SubAssign for PageCount {
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
