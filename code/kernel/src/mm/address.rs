use core::fmt::{self, Debug, Formatter};

use crate::config::{MEMORY_SIZE, PAGE_SIZE, PAGE_SIZE_BITS, PHYSICAL_MEMORY_OFFSET};

use super::page_table::PageTableEntry;

const PA_WIDTH_SV39: usize = 56;
const VA_WIDTH_SV39: usize = 39;
const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;
const VPN_WIDTH_SV39: usize = VA_WIDTH_SV39 - PAGE_SIZE_BITS;

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirAddr(usize);

///
/// PhyAddr can't deref, need to into PhyAddrRef
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddr(usize);

/// direct mapping to physical address
///
/// same as PhyAddr + PHYSICAL_MEMORY_OFFSET
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddrRef(usize);

/// assert self & 0xfff = 0
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirAddrMasked(usize);

/// assert self & 0xfff = 0
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddrMasked(usize);

/// assert self & 0xfff = 0
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhyAddrRefMasked(usize);

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
impl Debug for PhyAddrMasked {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA masked:{:#x}", self.0))
    }
}
impl Debug for VirAddrMasked {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA masked:{:#x}", self.0))
    }
}
impl Debug for PhyAddrRefMasked {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA ref masked:{:#x}", self.0))
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
    debug_check!(v < MEMORY_SIZE, "into PhyAddrRef error: {}", v)
);
impl_from_usize!(
    PhyAddrRef,
    v,
    Self(v),
    debug_check!(
        v - PHYSICAL_MEMORY_OFFSET < MEMORY_SIZE,
        "into PhyAddr error: {:x}",
        v
    )
);

macro_rules! impl_usize_from {
    ($name: ident, $v: ident, $body: stmt) => {
        impl From<$name> for usize {
            fn from($v: $name) -> Self {
                $body
            }
        }
    };
}

impl_usize_from!(VirAddr, v, v.0);
impl_usize_from!(PhyAddr, v, v.0);
impl_usize_from!(PhyAddrRef, v, v.0);
impl_usize_from!(VirAddrMasked, v, v.0);
impl_usize_from!(PhyAddrMasked, v, v.0);
impl_usize_from!(PhyAddrRefMasked, v, v.0);

macro_rules! impl_addr_masked_common {
    ($name: ident, $mask_name: ident) => {
        impl $name {
            pub const fn floor(&self) -> $mask_name {
                $mask_name(self.0 & !(PAGE_SIZE - 1))
            }
            pub const fn ceil(&self) -> $mask_name {
                $mask_name((self.0 + PAGE_SIZE - 1) & !(PAGE_SIZE - 1))
            }
            pub const fn page_offset(&self) -> usize {
                self.0 & (PAGE_SIZE - 1)
            }
            pub const fn aligned(&self) -> bool {
                self.page_offset() == 0
            }
        }
        impl From<$name> for $mask_name {
            fn from(v: $name) -> Self {
                v.floor()
            }
        }
        impl From<$mask_name> for $name {
            fn from(v: $mask_name) -> Self {
                Self(v.0)
            }
        }
    };
}

impl_addr_masked_common!(VirAddr, VirAddrMasked);
impl_addr_masked_common!(PhyAddr, PhyAddrMasked);
impl_addr_masked_common!(PhyAddrRef, PhyAddrRefMasked);

macro_rules! impl_phy_ref_translate {
    ($phy_name: ident, $phy_ref_name: ident) => {
        impl From<$phy_name> for $phy_ref_name {
            fn from(v: $phy_name) -> Self {
                Self(usize::from(v) + PHYSICAL_MEMORY_OFFSET)
            }
        }
        impl From<$phy_ref_name> for $phy_name {
            fn from(v: $phy_ref_name) -> Self {
                Self(usize::from(v) - PHYSICAL_MEMORY_OFFSET)
            }
        }
    };
}

impl_phy_ref_translate!(PhyAddr, PhyAddrRef);
impl_phy_ref_translate!(PhyAddrMasked, PhyAddrRefMasked);

impl VirAddrMasked {
    pub fn indexes(&self) -> [usize; 3] {
        let v = self.vpn();
        [v, v >> 9, v >> 18].map(|a| a & 0x1ff)
    }
    pub fn vpn(&self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    pub fn sub_one_page(&self) -> Self {
        Self(self.0 - PAGE_SIZE)
    }
}
impl PhyAddrMasked {
    pub fn ppn(&self) -> usize {
        self.0 >> PAGE_SIZE_BITS
    }
    pub fn from_satp(satp: usize) -> Self {
        // let ppn = satp & (1usize << 44) - 1;
        let ppn = satp & (1usize << 38) - 1;
        Self(ppn << 12)
    }
    pub unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
}

impl Default for PhyAddrRefMasked {
    fn default() -> Self {
        Self(PHYSICAL_MEMORY_OFFSET)
    }
}

impl PhyAddrRefMasked {
    pub fn get_pte_array(&self) -> &'static [PageTableEntry; 512] {
        self.get_mut()
    }
    pub fn get_bytes_array(&self) -> &'static [u8; 4096] {
        self.get_mut()
    }
    pub fn get_pte_array_mut(&mut self) -> &'static mut [PageTableEntry; 512] {
        self.get_mut()
    }
    pub fn get_bytes_array_mut(&mut self) -> &'static mut [u8; 4096] {
        self.get_mut()
    }
    pub fn get_mut<T>(&self) -> &'static mut T {
        let pa: PhyAddrRef = (*self).into();
        unsafe { &mut *(pa.0 as *mut T) }
    }
    pub unsafe fn from_usize(n: usize) -> Self {
        Self(n)
    }
    pub fn add_n_pg(&self, n: usize) -> Self {
        unsafe { Self::from_usize(usize::from(*self) + n * PAGE_SIZE) }
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

impl_step_by_one!(VirAddrMasked);
impl_step_by_one!(PhyAddrMasked);
impl_step_by_one!(PhyAddrRefMasked);

#[derive(Copy, Clone)]
pub struct SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    l: T,
    r: T,
}
impl<T> SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    pub fn new(start: T, end: T) -> Self {
        assert!(start <= end, "start {:?} > end {:?}!", start, end);
        Self { l: start, r: end }
    }
    pub fn get_start(&self) -> T {
        self.l
    }
    pub fn get_end(&self) -> T {
        self.r
    }
}
impl<T> IntoIterator for SimpleRange<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    type Item = T;
    type IntoIter = SimpleRangeIterator<T>;
    fn into_iter(self) -> Self::IntoIter {
        SimpleRangeIterator::new(self.l, self.r)
    }
}
pub struct SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    current: T,
    end: T,
}
impl<T> SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    pub fn new(l: T, r: T) -> Self {
        Self { current: l, end: r }
    }
}
impl<T> Iterator for SimpleRangeIterator<T>
where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug,
{
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.end {
            None
        } else {
            let t = self.current;
            self.current.step();
            Some(t)
        }
    }
}
pub type VPNRange = SimpleRange<VirAddrMasked>;
