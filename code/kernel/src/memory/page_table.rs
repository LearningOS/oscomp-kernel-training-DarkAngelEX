// #![allow(dead_code)]
use core::{fmt::Debug, slice::SlicePattern};

use alloc::vec::Vec;
use bitflags::bitflags;

use crate::{
    config::{
        DIRECT_MAP_BEGIN, DIRECT_MAP_END, INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP, PAGE_SIZE,
    },
    debug::PRINT_MAP_ALL,
    memory::asid,
    riscv::{
        self,
        register::csr,
        sfence::{self, sfence_vma_all_global},
    },
    tools::{allocator::TrackerAllocator, error::FrameOutOfMemory},
};

use super::{
    address::{
        PageCount, PhyAddr, PhyAddr4K, PhyAddrRef4K, StepByOne, UserAddr4K, VirAddr, VirAddr4K,
    },
    allocator::frame::{self, iter::FrameDataIter, FrameAllocator},
    asid::{Asid, AsidInfoTracker},
    user_space::UserArea,
};

static mut KERNEL_GLOBAL: Option<PageTable> = None;

bitflags! {
    // riscv-privileged 4.3.1 P87
    pub struct PTEFlags: u8 {
        const V = 1 << 0; // valid
        const R = 1 << 1; // readable
        const W = 1 << 2; // writalbe
        const X = 1 << 3; // executable
        const U = 1 << 4; // user mode
        const G = 1 << 5; // global mapping
        const A = 1 << 6; // access, set to 1 after r/w/x
        const D = 1 << 7; // dirty, set to 1 after write
        // RSW 2bit, reserved
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
pub struct PageTableEntry {
    bits: usize,
}

impl PageTableEntry {
    pub fn new(pa_4k: PhyAddr4K, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: (usize::from(pa_4k) >> 2) & ((1 << 54usize) - 1) | flags.bits as usize,
        }
    }
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// this function will clear reserved bit in [63:54]
    pub fn phy_addr(&self) -> PhyAddr4K {
        // (self.bits >> 10 & ((1usize << 44) - 1)).into()
        let mask = ((1usize << 44) - 1) << 10;
        unsafe { PhyAddr4K::from_usize((self.bits & mask) << 2) }
    }
    pub fn flags(&self) -> PTEFlags {
        // PTEFlags::from_bits(self.bits as u8).unwrap()
        unsafe { PTEFlags::from_bits_unchecked(self.bits as u8) }
    }
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    pub fn is_directory(&self) -> bool {
        self.is_valid()
            && (self.flags() & (PTEFlags::R | PTEFlags::W | PTEFlags::X)) == PTEFlags::empty()
    }
    pub fn is_leaf(&self) -> bool {
        self.is_valid()
            && (self.flags() & (PTEFlags::R | PTEFlags::W | PTEFlags::X)) != PTEFlags::empty()
    }
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
    pub fn is_user(&self) -> bool {
        (self.flags() & PTEFlags::U) != PTEFlags::empty()
    }
    pub fn rsw_8(&self) -> bool {
        (self.bits & 1usize << 8) != 0
    }
    pub fn rsw_9(&self) -> bool {
        (self.bits & 1usize << 9) != 0
    }
    pub fn reserved_bit(&self) -> usize {
        self.bits & ((1usize << 10 - 1) << 54)
    }
    pub fn alloc_by(
        &mut self,
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOutOfMemory> {
        assert!(!self.is_valid(), "try alloc to a valid pte");
        let pa = allocator.alloc()?.consume();
        pa.as_pte_array_mut()
            .iter_mut()
            .for_each(|x| *x = PageTableEntry::empty());
        *self = Self::new(PhyAddr4K::from(pa), flags | PTEFlags::V);
        Ok(())
    }
    #[deprecated = "replace by alloc_by"]
    pub fn alloc(&mut self, flags: PTEFlags) -> Result<(), FrameOutOfMemory> {
        self.alloc_by(flags, &mut frame::defualt_allocator())
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc_by(&mut self, allocator: &mut impl FrameAllocator) {
        assert!(self.is_valid());
        allocator.dealloc(self.phy_addr().into());
        *self = Self::empty();
    }
    #[deprecated = "replace by dealloc_by"]
    pub unsafe fn dealloc(&mut self) {
        self.dealloc_by(&mut frame::defualt_allocator())
    }
}

impl Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("PTE:{:#x}", self.bits))
    }
}

#[derive(Debug)]
pub struct PageTable {
    asid_tracker: AsidInfoTracker,
    satp: usize, // [63:60 MODE, 8 is SV39][59:44 ASID][43:0 PPN]
}

impl Drop for PageTable {
    fn drop(&mut self) {
        assert!(self.satp != 0);
        let allocator = &mut frame::defualt_allocator();
        self.free_user_directory_all(allocator);
        unsafe { allocator.dealloc(self.root_pa().into_ref()) };
        self.satp = 0;
        memory_trace!("PageTable::drop end");
    }
}

impl PageTable {
    /// asid set to zero must be success.
    pub fn new_empty(asid_tracker: AsidInfoTracker) -> Result<Self, FrameOutOfMemory> {
        let phy_ptr = frame::alloc_dpa()?.consume();
        let arr = phy_ptr.into_ref().as_pte_array_mut();
        arr.iter_mut()
            .for_each(|pte| *pte = PageTableEntry::empty());
        let asid = asid_tracker.asid().into_usize();
        Ok(PageTable {
            asid_tracker,
            satp: 8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn(),
        })
    }
    pub fn from_global(asid_tracker: AsidInfoTracker) -> Result<Self, FrameOutOfMemory> {
        memory_trace!("PageTable::from_global");
        let phy_ptr = frame::alloc_dpa()?.consume();
        let arr = phy_ptr.into_ref().as_pte_array_mut();
        let src = unsafe { KERNEL_GLOBAL.as_ref().unwrap() }
            .root_pa()
            .into_ref()
            .as_pte_array();
        arr[256..].copy_from_slice(&src[256..]);
        arr[..256]
            .iter_mut()
            .for_each(|pte| *pte = PageTableEntry::empty());
        let asid = asid_tracker.asid().into_usize();
        Ok(PageTable {
            asid_tracker,
            satp: 8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn(),
        })
    }
    pub fn drop_by(mut self, allocator: &mut impl FrameAllocator) {
        assert!(self.satp != 0);
        self.free_user_directory_all(allocator);
        unsafe { allocator.dealloc(self.root_pa().into_ref()) };
        self.satp = 0;
        // skip satp check
        core::mem::forget(self);
    }
    pub fn satp(&self) -> usize {
        self.satp
    }
    pub fn update_asid(&mut self, asid_tracker: AsidInfoTracker) {
        let asid = asid_tracker.asid().into_usize();
        self.asid_tracker = asid_tracker;
        self.satp = (self.satp & !(0xffff << 44)) | (asid & 0xffff) << 44
    }
    unsafe fn set_asid_in_satp(&mut self, asid: Asid) {
        self.satp = (self.satp & !(0xffff << 44)) | (asid.into_usize() & 0xffff) << 44
    }
    /// need sync with said_tracker
    pub unsafe fn set_satp(&mut self, satp: usize) {
        self.satp = satp
    }
    pub unsafe fn set_satp_register_uncheck(&self) {
        csr::set_satp(self.satp)
    }
    pub fn using(&mut self) {
        self.version_check();
        unsafe { self.set_satp_register_uncheck() }
    }
    pub fn version_check(&mut self) {
        match self.asid_tracker.version_check() {
            Ok(_) => (),
            Err(asid) => unsafe { self.set_asid_in_satp(asid) },
        }
    }
    pub fn root_pa(&self) -> PhyAddr4K {
        PhyAddr4K::from_satp(self.satp)
    }
    fn find_pte_create(
        &mut self,
        va: VirAddr4K,
        allocator: &mut impl FrameAllocator,
    ) -> Result<&mut PageTableEntry, FrameOutOfMemory> {
        let idxs = va.indexes();
        let mut par: PhyAddrRef4K = self.root_pa().into();
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut par.as_pte_array_mut()[idx];
            if i == 2 {
                return Ok(pte);
            }
            if !pte.is_valid() {
                pte.alloc_by(PTEFlags::V, allocator)?;
            }
            par = pte.phy_addr().into();
        }
        unreachable!()
    }
    fn find_pte(&self, va: VirAddr4K) -> Option<&mut PageTableEntry> {
        let idxs = va.indexes();
        let mut par: PhyAddrRef4K = self.root_pa().into();
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut par.as_pte_array_mut()[idx];
            if i == 2 {
                return Some(pte);
            }
            if !pte.is_valid() {
                // println!("find_pte err! {:?} {:?}", pte, pte.phy_addr());
                return None;
            }
            par = pte.phy_addr().into();
        }
        unreachable!()
    }
    /// return Err if out of memory
    pub fn map_user_range(
        &mut self,
        map_area: &UserArea,
        data_iter: &mut impl FrameDataIter,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOutOfMemory> {
        memory_trace!("PageTable::map_user_range");
        assert!(map_area.perm().contains(PTEFlags::U));
        let ubegin = map_area.begin();
        let uend = map_area.end();
        if ubegin == uend {
            return Ok(());
        }
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let flags = map_area.perm();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        return match map_user_range_0(ptes, l, r, flags, data_iter, allocator, ubegin) {
            Ok(ua) => {
                debug_check_eq!(ua, uend);
                Ok(())
            }
            Err(ua) => {
                // realease page table
                let alloc_area = UserArea::new(ubegin, ua, flags);
                self.unmap_user_range(&alloc_area, allocator);
                Err(FrameOutOfMemory)
            }
        };

        /// return value:
        ///
        /// Ok: next ua
        ///
        /// Err: err ua, There is no space assigned to this location
        #[inline(always)]
        fn map_user_range_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            flags: PTEFlags,
            data_iter: &mut impl FrameDataIter,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> Result<UserAddr4K, UserAddr4K> {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                memory_trace!("PageTable::map_user_range_0-0");
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if !pte.is_valid() {
                    pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                }
                let ptes = PageTable::ptes_from_pte(pte);
                ua = map_user_range_1(ptes, l, r, flags, data_iter, allocator, ua)?;
            }
            Ok(ua)
        }
        #[inline(always)]
        fn map_user_range_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            flags: PTEFlags,
            data_iter: &mut impl FrameDataIter,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> Result<UserAddr4K, UserAddr4K> {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                memory_trace!("PageTable::map_user_range_1-0");
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if !pte.is_valid() {
                    pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                }
                let ptes = PageTable::ptes_from_pte(pte);
                ua = map_user_range_2(ptes, l, r, flags, data_iter, allocator, ua)?;
            }
            Ok(ua)
        }
        #[inline(always)]
        fn map_user_range_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            flags: PTEFlags,
            data_iter: &mut impl FrameDataIter,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> Result<UserAddr4K, UserAddr4K> {
            for pte in &mut ptes[l[0]..=r[0]] {
                assert!(!pte.is_valid(), "remap of {:?}", ua);
                memory_trace!("PageTable::map_user_range_2-0");
                let par = allocator.alloc().map_err(|_| ua)?.consume();
                memory_trace!("PageTable::map_user_range_2-1");
                // fill zero if return Error
                let _ = data_iter.write_to(par.as_bytes_array_mut());
                memory_trace!("PageTable::map_user_range_2-2");
                *pte = PageTableEntry::new(par.into(), flags | PTEFlags::V);
                ua = ua.add_one_page();
            }
            Ok(ua)
        }
    }

    pub fn unmap_user_range(&mut self, map_area: &UserArea, allocator: &mut impl FrameAllocator) {
        assert!(map_area.perm().contains(PTEFlags::U));
        let ubegin = map_area.begin();
        let uend = map_area.end();
        if ubegin == uend {
            return;
        }
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let ua = unmap_user_range_0(ptes, l, r, allocator, ubegin);
        debug_check_eq!(ua, uend);
        return;

        #[inline(always)]
        fn unmap_user_range_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> UserAddr4K {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                let ptes = PageTable::ptes_from_pte(pte);
                ua = unmap_user_range_1(ptes, l, r, allocator, ua);
                if full {
                    unsafe { pte.dealloc_by(allocator) };
                }
            }
            ua
        }
        #[inline(always)]
        fn unmap_user_range_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> UserAddr4K {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                let ptes = PageTable::ptes_from_pte(pte);
                ua = unmap_user_range_2(ptes, l, r, allocator, ua);
                if full {
                    unsafe { pte.dealloc_by(allocator) };
                }
            }
            ua
        }
        #[inline(always)]
        fn unmap_user_range_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> UserAddr4K {
            for pte in &mut ptes[l[0]..=r[0]].iter_mut() {
                assert!(pte.is_leaf(), "unmap invalid leaf: {:?}", ua);
                unsafe { pte.dealloc_by(allocator) };
                ua = ua.add_one_page();
            }
            ua
        }
    }
    pub fn unmap_user_range_lazy(
        &mut self,
        map_area: &UserArea,
        allocator: &mut impl FrameAllocator,
    ) -> PageCount {
        assert!(map_area.perm().contains(PTEFlags::U));
        let ubegin = map_area.begin();
        let uend = map_area.end();
        let page_count = PageCount::from_usize(0);
        if ubegin == uend {
            return page_count;
        }
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let (page_count, ua) = unmap_user_range_lazy_0(ptes, l, r, page_count, allocator, ubegin);
        debug_check_eq!(ua, uend);
        return page_count;

        #[inline(always)]
        fn unmap_user_range_lazy_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            mut page_count: PageCount,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> (PageCount, UserAddr4K) {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                    let ptes = PageTable::ptes_from_pte(pte);
                    (page_count, ua) =
                        unmap_user_range_lazy_1(ptes, l, r, page_count, allocator, ua);
                    if full {
                        unsafe { pte.dealloc_by(allocator) };
                    }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            (page_count, ua)
        }
        #[inline(always)]
        fn unmap_user_range_lazy_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            mut page_count: PageCount,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> (PageCount, UserAddr4K) {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                    let ptes = PageTable::ptes_from_pte(pte);
                    (page_count, ua) =
                        unmap_user_range_lazy_2(ptes, l, r, page_count, allocator, ua);
                    if full {
                        unsafe { pte.dealloc_by(allocator) };
                    }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            (page_count, ua)
        }
        #[inline(always)]
        fn unmap_user_range_lazy_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            mut page_count: PageCount,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> (PageCount, UserAddr4K) {
            for pte in &mut ptes[l[0]..=r[0]].iter_mut() {
                if pte.is_valid() {
                    assert!(pte.is_leaf(), "unmap invalid leaf: {:?}", ua);
                    unsafe { pte.dealloc_by(allocator) };
                    page_count += PageCount::from_usize(0);
                }
                ua = ua.add_one_page();
            }
            (page_count, ua)
        }
    }
    pub fn map_direct_range(
        &mut self,
        vbegin: VirAddr4K,
        pbegin: PhyAddrRef4K,
        size: usize,
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOutOfMemory> {
        if size == 0 {
            return Ok(());
        }
        assert!(size % PAGE_SIZE == 0);
        let par = self.root_pa().into_ref();
        let vend = unsafe { VirAddr4K::from_usize(usize::from(vbegin) + size) };
        let l = &vbegin.indexes();
        let r = &vend.sub_one_page().indexes();
        if PRINT_MAP_ALL {
            println!(
                "map_range: {:#x} - {:#x} size = {}",
                usize::from(vbegin),
                usize::from(vend),
                size
            );
            println!("l:{:?}", l);
            println!("r:{:?}", r);
        }
        let ptes = par.as_pte_array_mut();
        // clear 12 + 9 * 3 = 39 bit
        let va = map_direct_range_0(ptes, l, r, flags, vbegin, pbegin.into(), allocator)?;
        debug_check_eq!(va, vend);
        return Ok(());

        #[inline(always)]
        fn map_direct_range_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            flags: PTEFlags,
            mut va: VirAddr4K,
            mut pa: PhyAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<VirAddr4K, FrameOutOfMemory> {
            // println!("level 0: {:?} {:?}-{:?}", va, l, r);
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    // 1GB page table
                    assert!(!pte.is_valid(), "1GB pagetable: remap");
                    debug_check!(va.into_usize() % (PAGE_SIZE * (1 << 9 * 2)) == 0);
                    // if true || PRINT_MAP_ALL {
                    //     println!("map 1GB {:?} -> {:?}", va, pa);
                    // }
                    *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                    unsafe {
                        va = VirAddr4K::from_usize(va.into_usize() + PAGE_SIZE * (1 << 9 * 2));
                        pa = PhyAddr4K::from_usize(pa.into_usize() + PAGE_SIZE * (1 << 9 * 2));
                    }
                } else {
                    if !pte.is_valid() {
                        pte.alloc_by(PTEFlags::V, allocator)?;
                    }
                    let ptes = PageTable::ptes_from_pte(pte);
                    (va, pa) = map_direct_range_1(ptes, l, r, flags, va, pa, allocator)?
                }
            }
            Ok(va)
        }
        #[inline(always)]
        fn map_direct_range_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            flags: PTEFlags,
            mut va: VirAddr4K,
            mut pa: PhyAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<(VirAddr4K, PhyAddr4K), FrameOutOfMemory> {
            // println!("level 1: {:?} {:?}-{:?}", va, l, r);
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    // 1MB page table
                    assert!(!pte.is_valid(), "1MB pagetable: remap");
                    debug_check!(va.into_usize() % (PAGE_SIZE * (1 << 9 * 1)) == 0);
                    *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                    unsafe {
                        va = VirAddr4K::from_usize(va.into_usize() + PAGE_SIZE * (1 << 9 * 1));
                        pa = PhyAddr4K::from_usize(pa.into_usize() + PAGE_SIZE * (1 << 9 * 1));
                    }
                } else {
                    if !pte.is_valid() {
                        pte.alloc_by(PTEFlags::V, allocator)?;
                    }
                    let ptes = PageTable::ptes_from_pte(pte);
                    (va, pa) = map_direct_range_2(ptes, l, r, flags, va, pa, allocator);
                }
            }
            Ok((va, pa))
        }
        #[inline(always)]
        fn map_direct_range_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            flags: PTEFlags,
            mut va: VirAddr4K,
            mut pa: PhyAddr4K,
            _allocator: &mut impl FrameAllocator,
        ) -> (VirAddr4K, PhyAddr4K) {
            // println!("level 2: {:?} {:?}-{:?}", va, l, r);
            for pte in &mut ptes[l[0]..=r[0]] {
                assert!(!pte.is_valid(), "remap of {:?} -> {:?}", va, pa);
                // if true || PRINT_MAP_ALL {
                //     println!("map: {:?} -> {:?}", va, pa);
                // }
                *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                unsafe {
                    va = VirAddr4K::from_usize(va.into_usize() + PAGE_SIZE);
                    pa = PhyAddr4K::from_usize(pa.into_usize() + PAGE_SIZE);
                }
            }
            (va, pa)
        }
    }

    /// clear [vbegin, vend)
    pub fn unmap_direct_range(&mut self, vbegin: VirAddr4K, vend: VirAddr4K) {
        assert!(vbegin <= vend, "free_range vbegin <= vend");
        if vbegin == vend {
            return;
        }
        let l = &vbegin.indexes();
        let r = &vend.sub_one_page().indexes();
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        return unmap_direct_range_0(ptes, l, r);

        #[inline(always)]
        fn unmap_direct_range_0(ptes: &mut [PageTableEntry; 512], l: &[usize; 3], r: &[usize; 3]) {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    // 1GB page table
                    debug_check!(pte.is_leaf());
                    *pte = PageTableEntry::empty();
                } else {
                    debug_check!(pte.is_directory());
                    let ptes = PageTable::ptes_from_pte(pte);
                    unmap_direct_range_1(ptes, l, r);
                }
            }
        }
        #[inline(always)]
        fn unmap_direct_range_1(ptes: &mut [PageTableEntry; 512], l: &[usize; 2], r: &[usize; 2]) {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    debug_check!(pte.is_leaf());
                    *pte = PageTableEntry::empty();
                } else {
                    debug_check!(pte.is_directory());
                    let ptes = PageTable::ptes_from_pte(pte);
                    unmap_direct_range_2(ptes, l, r);
                }
            }
        }
        #[inline(always)]
        fn unmap_direct_range_2(ptes: &mut [PageTableEntry; 512], l: &[usize; 1], r: &[usize; 1]) {
            for pte in &mut ptes[l[0]..=r[0]] {
                debug_check!(pte.is_leaf());
                *pte = PageTableEntry::empty();
            }
        }
    }

    /// if exists valid leaf, it will panic.
    pub fn free_user_directory_all(&mut self, allocator: &mut impl FrameAllocator) {
        let ubegin = UserAddr4K::null();
        let uend = UserAddr4K::user_max();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let ptes = self.root_pa().into_ref().as_pte_array_mut();

        let ua = free_user_directory_all_0(ptes, l, r, ubegin, allocator);
        assert_eq!(ua, uend);
        return;
        #[inline(always)]
        fn free_user_directory_all_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> UserAddr4K {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    debug_check!(
                        pte.is_directory(),
                        "free_user_directory_all: need directory but leaf"
                    );
                    let ptes = PageTable::ptes_from_pte(pte);
                    ua = free_user_directory_all_1(ptes, l, r, ua, allocator);
                    unsafe { pte.dealloc_by(allocator) }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r))
                }
            }
            ua
        }
        #[inline(always)]
        fn free_user_directory_all_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> UserAddr4K {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    debug_check!(
                        pte.is_directory(),
                        "free_user_directory_all: need directory but leaf"
                    );
                    let ptes = PageTable::ptes_from_pte(pte);
                    ua = free_user_directory_all_2(ptes, l, r, ua, allocator);
                    unsafe { pte.dealloc_by(allocator) }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r))
                }
            }
            ua
        }
        #[inline(always)]
        fn free_user_directory_all_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            mut ua: UserAddr4K,
            _allocator: &mut impl FrameAllocator,
        ) -> UserAddr4K {
            for pte in &mut ptes[l[0]..=r[0]] {
                assert!(
                    !pte.is_valid(),
                    "free_user_directory_all: exist valid leaf: {:?}",
                    ua
                );
                ua = ua.add_one_page();
            }
            ua
        }
    }

    #[inline(always)]
    fn next_lr<'a, 'b, const N1: usize, const N: usize>(
        l: &'a [usize; N1],
        r: &'b [usize; N1],
        xbegin: &'a [usize; N],
        xend: &'b [usize; N],
        i: usize,
    ) -> (&'a [usize; N], &'b [usize; N], bool) {
        let xl = if i == 0 {
            l.rsplit_array_ref::<N>().1
        } else {
            xbegin
        };
        let xr = if i == r[0] - l[0] {
            r.rsplit_array_ref::<N>().1
        } else {
            xend
        };
        (xl, xr, xl.eq(xbegin) && xr.eq(xend))
    }
    #[inline(always)]
    fn ptes_from_pte(pte: &mut PageTableEntry) -> &'static mut [PageTableEntry; 512] {
        debug_check!(pte.is_directory());
        PhyAddrRef4K::from(pte.phy_addr()).as_pte_array_mut()
    }
    fn indexes_diff<const N: usize>(begin: &[usize; N], end: &[usize; N]) -> PageCount {
        fn get_num<const N: usize>(a: &[usize; N]) -> usize {
            let mut value = 0;
            for &x in a {
                value <<= 9;
                value += x;
            }
            value
        }
        let x0 = get_num(begin);
        let x1 = get_num(end) + 1;
        PageCount::from_usize(x1 - x0)
    }

    /// if return Err, frame exhausted.
    pub fn map_par(
        &mut self,
        va: VirAddr4K,
        par: PhyAddrRef4K,
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOutOfMemory> {
        let pte = self.find_pte_create(va, allocator)?;
        debug_check!(!pte.is_valid(), "va {:?} is mapped before mapping", va);
        *pte = PageTableEntry::new(par.into(), flags | PTEFlags::V);
        Ok(())
    }
    /// don't release space of par.
    pub fn unmap_par(&mut self, va: VirAddr4K, par: PhyAddrRef4K) {
        let pte = self.find_pte(va).expect("unmap invalid virtual address!");
        assert!(pte.is_valid(), "pte {:?} is invalid before unmapping", pte);
        assert!(pte.phy_addr().into_ref() == par);
        *pte = PageTableEntry::empty();
    }
    pub fn translate(&self, va: VirAddr4K) -> Option<PageTableEntry> {
        self.find_pte(va).map(|pte| *pte)
    }
    pub unsafe fn translate_uncheck(&self, va: VirAddr4K) -> PageTableEntry {
        *self
            .find_pte(va)
            .unwrap_or_else(|| panic!("translate_uncheck: invalid pte from {:?}", va))
    }
    pub fn copy_kernel_from(&mut self, src: &Self) {
        let src = src.root_pa().into_ref().as_pte_array();
        let dst = self.root_pa().into_ref().as_pte_array_mut();
        dst[0..256].copy_from_slice(&src[0..256]);
        // dst.array_chunks_mut::<256>();
    }
    /// copy kernel segment
    ///
    /// alloc new space for user
    pub fn fork(&mut self, allocator: &mut impl FrameAllocator) -> Result<Self, FrameOutOfMemory> {
        memory_trace!("PageTable::fork begin");
        let mut pt = Self::from_global(asid::alloc_asid())?;
        // println!("PageTable::fork {:#x}", self as *const Self as usize);
        Self::copy_user_range_lazy(
            &mut pt,
            self,
            &UserArea::new(UserAddr4K::null(), UserAddr4K::user_max(), PTEFlags::U),
            allocator,
        )?;
        println!("PageTable::fork end");
        Ok(pt)
    }
    /// lazy copy all range, skip invalid leaf.
    fn copy_user_range_lazy(
        dst: &mut Self,
        src: &mut Self,
        map_area: &UserArea,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOutOfMemory> {
        memory_trace!("copy_user_range_lazy");
        map_area.user_assert();
        let ubegin = map_area.begin();
        let uend = map_area.end();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let src_ptes = src.root_pa().into_ref().as_pte_array_mut();
        let dst_ptes = dst.root_pa().into_ref().as_pte_array_mut();
        return match copy_user_range_lazy_0(dst_ptes, src_ptes, l, r, ubegin, allocator) {
            Ok(ua) => {
                debug_check_eq!(ua, uend);
                Ok(())
            }
            Err(ua) => {
                let alloc_area = UserArea::new(ubegin, ua, PTEFlags::U);
                dst.unmap_user_range_lazy(&alloc_area, allocator);
                Err(FrameOutOfMemory)
            }
        };
        #[inline(always)]
        fn copy_user_range_lazy_0(
            dst_ptes: &mut [PageTableEntry; 512],
            src_ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<UserAddr4K, UserAddr4K> {
            memory_trace!("copy_user_range_lazy_0");
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            let dst_it = dst_ptes[l[0]..=r[0]].iter_mut();
            let src_it = src_ptes[l[0]..=r[0]].iter_mut();
            for (i, (dst_pte, src_pte)) in &mut dst_it.zip(src_it).enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if src_pte.is_valid() {
                    assert!(src_pte.is_directory());
                    memory_trace!("copy_user_range_lazy_0 0");
                    dst_pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                    memory_trace!("copy_user_range_lazy_0 1");
                    let dst_ptes = PageTable::ptes_from_pte(dst_pte);
                    let src_ptes = PageTable::ptes_from_pte(src_pte);
                    ua = copy_user_range_lazy_1(dst_ptes, src_ptes, l, r, ua, allocator)?;
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            Ok(ua)
        }
        #[inline(always)]
        fn copy_user_range_lazy_1(
            dst_ptes: &mut [PageTableEntry; 512],
            src_ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<UserAddr4K, UserAddr4K> {
            memory_trace!("copy_user_range_lazy_1");
            // println!("lazy_1 ua: {:#x}", ua.into_usize());
            let xbegin = &[0];
            let xend = &[511];
            let dst_it = dst_ptes[l[0]..=r[0]].iter_mut();
            let src_it = src_ptes[l[0]..=r[0]].iter_mut();
            for (i, (dst_pte, src_pte)) in &mut dst_it.zip(src_it).enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if src_pte.is_valid() {
                    assert!(src_pte.is_directory());
                    dst_pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                    let dst_ptes = PageTable::ptes_from_pte(dst_pte);
                    let src_ptes = PageTable::ptes_from_pte(src_pte);
                    ua = copy_user_range_lazy_2(dst_ptes, src_ptes, l, r, ua, allocator)?;
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            Ok(ua)
        }
        #[inline(always)]
        fn copy_user_range_lazy_2(
            dst_ptes: &mut [PageTableEntry; 512],
            src_ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<UserAddr4K, UserAddr4K> {
            memory_trace!("copy_user_range_lazy_2");
            let dst_it = dst_ptes[l[0]..=r[0]].iter_mut();
            let src_it = src_ptes[l[0]..=r[0]].iter_mut();
            for (dst_pte, src_pte) in &mut dst_it.zip(src_it) {
                if src_pte.is_valid() {
                    assert!(src_pte.is_leaf() && src_pte.is_user());
                    let perm =
                        src_pte.flags() & (PTEFlags::U | PTEFlags::R | PTEFlags::W | PTEFlags::X);
                    dst_pte.alloc_by(perm, allocator).map_err(|_| ua)?;
                    let src = src_pte.phy_addr().into_ref().as_usize_array();
                    let dst = dst_pte.phy_addr().into_ref().as_usize_array_mut();
                    dst[0..512].copy_from_slice(&src[0..512]);
                    memory_trace!("copy_user_range_lazy_2");
                }
                ua = ua.add_one_page();
            }
            memory_trace!("copy_user_range_lazy_2");
            Ok(ua)
        }
    }
}

/// new a kernel page table
/// set asid to 0.
/// if return None, means no enough memory.
fn new_kernel_page_table() -> Result<PageTable, FrameOutOfMemory> {
    #[allow(dead_code)]
    extern "C" {
        // kernel segment ALIGN 4K
        fn stext();
        fn etext();
        // read only data segment ALIGN 4K
        fn srodata();
        fn erodata();
        // writable data segment ALIGN 4K
        fn sdata();
        fn edata();
        // stack ALIGN 4K
        fn sstack();
        fn estack();
        // bss ALIGN 4K
        fn sbss();
        fn ebss();
        // end ALIGN 4K
        fn end();
    }
    let mut page_table = PageTable::new_empty(asid::alloc_asid())?;
    fn get_usize_va(va: usize) -> VirAddr4K {
        unsafe { VirAddr4K::from_usize(va) }
    }
    fn get_usize_pa(pa: usize) -> PhyAddrRef4K {
        unsafe { PhyAddrRef4K::from_usize(pa) }
    }
    fn get_va(xva: usize) -> VirAddr4K {
        get_usize_va(xva)
    }
    fn pa_from_kernel(xva: usize) -> PhyAddrRef4K {
        get_usize_pa(xva - KERNEL_OFFSET_FROM_DIRECT_MAP)
    }
    fn pa_from_dirref(xva: usize) -> PhyAddrRef4K {
        get_usize_pa(xva)
    }
    fn get_size(b: usize, e: usize) -> usize {
        assert!(b % PAGE_SIZE == 0);
        assert!(e % PAGE_SIZE == 0);
        e.checked_sub(b).unwrap()
    }
    fn xmap_fn(
        pt: &mut PageTable,
        b: usize,
        e: usize,
        flags: PTEFlags,
        pa_fn: impl FnOnce(usize) -> PhyAddrRef4K,
        allocator: &mut impl FrameAllocator,
    ) {
        pt.map_direct_range(get_va(b), pa_fn(b), get_size(b, e), flags, allocator)
            .unwrap();
    }
    fn xmap_impl_kernel(
        pt: &mut PageTable,
        b: usize,
        e: usize,
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) {
        pt.map_direct_range(
            get_va(b),
            pa_from_kernel(b),
            get_size(b, e),
            flags,
            allocator,
        )
        .unwrap();
    }
    fn xmap_impl_dirref(
        pt: &mut PageTable,
        b: usize,
        e: usize,
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) {
        pt.map_direct_range(
            get_va(b),
            pa_from_dirref(b),
            get_size(b, e),
            flags,
            allocator,
        )
        .unwrap();
    }
    fn xmap_kernel(
        pt: &mut PageTable,
        b: unsafe extern "C" fn(),
        e: unsafe extern "C" fn(),
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) {
        xmap_impl_kernel(pt, b as usize, e as usize, flags, allocator);
    }
    let execable = PTEFlags::G | PTEFlags::R | PTEFlags::X;
    let readonly = PTEFlags::G | PTEFlags::R;
    let writable = PTEFlags::G | PTEFlags::R | PTEFlags::W;
    let allocator = &mut frame::defualt_allocator();
    xmap_kernel(&mut page_table, stext, etext, execable, allocator);
    xmap_kernel(&mut page_table, srodata, erodata, readonly, allocator);
    // xmap_kernel(&mut page_table, sdata, edata, writable);
    // xmap_kernel(&mut page_table, sstack, estack, writable);
    // xmap_kernel(&mut page_table, sbss, ebss, writable);
    xmap_kernel(&mut page_table, sdata, ebss, writable, allocator);
    // memory used in init frame.
    xmap_impl_kernel(
        &mut page_table,
        end as usize,
        INIT_MEMORY_END,
        writable,
        allocator,
    );
    // direct map
    println!("map DIRECT_MAP");
    xmap_impl_dirref(
        &mut page_table,
        DIRECT_MAP_BEGIN,
        DIRECT_MAP_END,
        writable,
        allocator,
    );
    Ok(page_table)
}

pub unsafe fn translated_byte_buffer_force(
    page_table: &PageTable,
    ptr: *const u8,
    len: usize,
) -> Vec<&'static mut [u8]> {
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirAddr::from(start);
        let mut va4k = start_va.floor();
        let par: PhyAddrRef4K = page_table.translate_uncheck(va4k).phy_addr().into(); // unsafe
        va4k.step();
        let end_va = VirAddr::from(va4k).min(VirAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut par.as_bytes_array_mut()[start_va.page_offset()..]);
        } else {
            v.push(&mut par.as_bytes_array_mut()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

fn direct_map_test() {
    unsafe {
        println!("direct_map_test");
        let a = INIT_MEMORY_END - 8;
        let ptr = a as *mut usize;
        let xptr = PhyAddrRef4K::from_usize(ptr as usize - KERNEL_OFFSET_FROM_DIRECT_MAP);
        *xptr.as_mut() = 1234usize;
        assert_eq!(*ptr, 1234);
    };
}

pub fn init_kernel_page_table() {
    println!("[FTL OS]init kerne page table");
    let new_satp = new_kernel_page_table().expect("new kernel page table error.");
    let satp = new_satp.satp();
    unsafe {
        assert!(
            KERNEL_GLOBAL.is_none(),
            "KERNEL_GLOBAL has been initialized"
        );
        KERNEL_GLOBAL = Some(new_satp);
        csr::set_satp(satp);
        sfence::sfence_vma_all_global();
        debug_run!({ direct_map_test() });
    }
}

/// used by another hart
pub fn set_satp_by_global() {
    unsafe {
        csr::set_satp(
            KERNEL_GLOBAL
                .as_ref()
                .expect("KERNEL_GLOBAL has not been initialized")
                .satp(),
        );
    }
    // sfence::sfence_vma_all_global();
}
