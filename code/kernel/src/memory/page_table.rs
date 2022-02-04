// #![allow(dead_code)]
use core::fmt::Debug;

use alloc::vec::Vec;
use bitflags::bitflags;

use crate::{
    config::{
        DIRECT_MAP_BEGIN, DIRECT_MAP_END, INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP, PAGE_SIZE,
    },
    debug::PRINT_MAP_ALL,
    memory::frame_allocator::frame_dealloc_dpa,
    riscv::csr,
};

use super::{
    address::{PhyAddr, PhyAddrMasked, PhyAddrRefMasked, StepByOne, VirAddr, VirAddrMasked},
    frame_allocator::{frame_alloc_dpa, FrameTrackerDpa},
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
    pub fn new(pa_mask: PhyAddrMasked, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: (usize::from(pa_mask) >> 2) & ((1 << 54usize) - 1) | flags.bits as usize,
        }
    }
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// this function will clear reserved bit in [63:54]
    pub fn phy_addr(&self) -> PhyAddrMasked {
        // (self.bits >> 10 & ((1usize << 44) - 1)).into()
        PhyAddr::from((self.bits & ((1usize << 54) - 1)) << 2).into()
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
    pub fn alloc_leaf(&mut self, pam: PhyAddrMasked, flags: PTEFlags) {
        assert!(!self.is_valid(), "try alloc to a valid pte");
        *self = Self::new(pam, flags | PTEFlags::V);
    }
    pub fn alloc_directory(&mut self, pam: PhyAddrMasked, flags: PTEFlags) {
        assert!(!self.is_valid(), "try alloc to a valid pte");
        *self = Self::new(pam, flags | PTEFlags::V);
    }
    pub fn alloc(&mut self, flags: PTEFlags) -> Result<(), ()> {
        assert!(!self.is_valid(), "try alloc to a valid pte");
        let pam = frame_alloc_dpa()?.consume();
        PhyAddrRefMasked::from(pam)
            .as_pte_array_mut()
            .iter_mut()
            .for_each(|x| *x = PageTableEntry::empty());
        *self = Self::new(pam, flags | PTEFlags::V);
        Ok(())
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc(&mut self) {
        assert!(self.is_valid());
        frame_dealloc_dpa(self.phy_addr());
        *self = Self::empty();
    }
}

impl Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("PTE:{:#x}", self.bits))
    }
}

struct PageTable {
    satp: usize, // [63:60 MODE, 8 is SV39][59:44 ASID][43:0 PPN]
}

// /// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// this function will not set all empty.
    ///
    /// asid set to zero must be success.
    pub fn new_empty(asid: usize) -> Result<Self, ()> {
        let phy_ptr = frame_alloc_dpa()?.consume();
        let arr = phy_ptr.into_ref().as_pte_array_mut();
        arr.iter_mut()
            .for_each(|pte| *pte = PageTableEntry::empty());
        Ok(PageTable {
            satp: 8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn(),
        })
    }
    pub fn from_global(asid: usize) -> Result<Self, ()> {
        let phy_ptr = frame_alloc_dpa()?.consume();
        let arr = phy_ptr.into_ref().as_pte_array_mut();
        let src = unsafe { KERNEL_GLOBAL.as_ref().unwrap_unchecked() }
            .root_pam()
            .into_ref()
            .as_pte_array();
        arr[..256].copy_from_slice(&src[..256]);
        arr[256..]
            .iter_mut()
            .for_each(|pte| *pte = PageTableEntry::empty());
        Ok(PageTable {
            satp: 8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn(),
        })
    }
    pub fn satp(&self) -> usize {
        self.satp
    }
    pub fn set_asid(&mut self, asid: usize) {
        self.satp = (self.satp & !(0xffff << 44)) | (asid & 0xffff) << 44
    }
    pub fn set_satp(&mut self, satp: usize) {
        self.satp = satp
    }
    /// Temporarily used to get arguments from user space.
    pub const fn from_satp(satp: usize) -> Self {
        Self { satp }
    }
    fn root_pam(&self) -> PhyAddrMasked {
        PhyAddrMasked::from_satp(self.satp)
    }
    fn find_pte_create(&mut self, vam: VirAddrMasked) -> Result<&mut PageTableEntry, ()> {
        let idxs = vam.indexes();
        let mut parm: PhyAddrRefMasked = self.root_pam().into();
        // auto release when fail
        let mut alloc_0: Option<FrameTrackerDpa> = None;
        let mut alloc_1: Option<FrameTrackerDpa> = None;
        let px = [&mut alloc_0, &mut alloc_1];

        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut parm.as_pte_array_mut()[idx];
            if i == 2 {
                core::mem::forget(alloc_0);
                core::mem::forget(alloc_1);
                // alloc_0.map(|a| a.consume());
                // alloc_1.map(|a| a.consume());
                return Ok(pte);
            }
            if !pte.is_valid() {
                let frame = frame_alloc_dpa()?; // alloc
                let data = frame.data();
                *px[i] = Some(frame);
                *pte = PageTableEntry::new(data, PTEFlags::V);
            }
            parm = pte.phy_addr().into();
        }
        unreachable!()
    }
    fn find_pte(&self, vam: VirAddrMasked) -> Option<&mut PageTableEntry> {
        let idxs = vam.indexes();
        let mut parm: PhyAddrRefMasked = self.root_pam().into();
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut parm.as_pte_array_mut()[idx];
            if i == 2 {
                return Some(pte);
            }
            if !pte.is_valid() {
                // println!("find_pte err! {:?} {:?}", pte, pte.phy_addr());
                return None;
            }
            parm = pte.phy_addr().into();
        }
        unreachable!()
    }

    pub fn map_range(
        &mut self,
        vbegin: VirAddrMasked,
        pbegin: PhyAddrRefMasked,
        size: usize,
        flags: PTEFlags,
    ) -> Result<&mut Self, ()> {
        if size == 0 {
            return Ok(self);
        }
        assert!(size % PAGE_SIZE == 0);
        let parm: PhyAddrRefMasked = self.root_pam().into();
        let vend = unsafe { VirAddrMasked::from_usize(usize::from(vbegin) + size) };
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
        // clear 12 + 9 * 3 = 39 bit
        Self::map_range_0(parm, l, r, flags, vbegin, pbegin.into())?;
        Ok(self)
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
    fn map_range_0(
        parm: PhyAddrRefMasked,
        l: &[usize; 3],
        r: &[usize; 3],
        flags: PTEFlags,
        mut va: VirAddrMasked,
        mut pa: PhyAddrMasked,
    ) -> Result<(), ()> {
        // println!("level 0: {:?} {:?}-{:?}", va, l, r);
        let xbegin = &[0, 0];
        let xend = &[511, 511];
        for (i, pte) in &mut parm.as_pte_array_mut()[l[0]..=r[0]].iter_mut().enumerate() {
            let (l, r, full) = Self::next_lr(l, r, xbegin, xend, i);
            if full {
                // 1GB page table
                assert!(!pte.is_valid(), "1GB pagetable: remap");
                debug_check!(va.into_usize() % (PAGE_SIZE * (1 << 9 * 2)) == 0);
                // if true || PRINT_MAP_ALL {
                //     println!("map 1GB {:?} -> {:?}", va, pa);
                // }
                *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                unsafe {
                    va = VirAddrMasked::from_usize(va.into_usize() + PAGE_SIZE * (1 << 9 * 2));
                    pa = PhyAddrMasked::from_usize(pa.into_usize() + PAGE_SIZE * (1 << 9 * 2));
                }
            } else {
                if !pte.is_valid() {
                    pte.alloc(PTEFlags::V)?;
                }
                (va, pa) = Self::map_range_1(pte, l, r, flags, va, pa)?
            }
        }
        Ok(())
    }
    #[inline(always)]
    fn map_range_1(
        pte: &mut PageTableEntry,
        l: &[usize; 2],
        r: &[usize; 2],
        flags: PTEFlags,
        mut va: VirAddrMasked,
        mut pa: PhyAddrMasked,
    ) -> Result<(VirAddrMasked, PhyAddrMasked), ()> {
        // println!("level 1: {:?} {:?}-{:?}", va, l, r);
        let xbegin = &[0];
        let xend = &[511];
        for (i, pte) in &mut PhyAddrRefMasked::from(pte.phy_addr()).as_pte_array_mut()[l[0]..=r[0]]
            .iter_mut()
            .enumerate()
        {
            let (l, r, full) = Self::next_lr(l, r, xbegin, xend, i);
            if full {
                // 1MB page table
                assert!(!pte.is_valid(), "1MB pagetable: remap");
                debug_check!(va.into_usize() % (PAGE_SIZE * (1 << 9 * 1)) == 0);
                *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                unsafe {
                    va = VirAddrMasked::from_usize(va.into_usize() + PAGE_SIZE * (1 << 9 * 1));
                    pa = PhyAddrMasked::from_usize(pa.into_usize() + PAGE_SIZE * (1 << 9 * 1));
                }
            } else {
                if !pte.is_valid() {
                    pte.alloc(PTEFlags::V)?;
                }
                (va, pa) = Self::map_range_2(pte, l, r, flags, va, pa);
            }
        }
        Ok((va, pa))
    }
    #[inline(always)]
    fn map_range_2(
        pte: &mut PageTableEntry,
        l: &[usize; 1],
        r: &[usize; 1],
        flags: PTEFlags,
        mut va: VirAddrMasked,
        mut pa: PhyAddrMasked,
    ) -> (VirAddrMasked, PhyAddrMasked) {
        // println!("level 2: {:?} {:?}-{:?}", va, l, r);
        for pte in &mut PhyAddrRefMasked::from(pte.phy_addr()).as_pte_array_mut()[l[0]..=r[0]] {
            assert!(!pte.is_valid(), "remap of {:?} -> {:?}", va, pa);
            // if true || PRINT_MAP_ALL {
            //     println!("map: {:?} -> {:?}", va, pa);
            // }
            *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
            unsafe {
                va = VirAddrMasked::from_usize(va.into_usize() + PAGE_SIZE);
                pa = PhyAddrMasked::from_usize(pa.into_usize() + PAGE_SIZE);
            }
        }
        (va, pa)
    }
    /// clear [vbegin, vend)
    pub fn unmap_range(&mut self, vbegin: VirAddrMasked, vend: VirAddrMasked) {
        assert!(vbegin <= vend, "free_range vbegin <= vend");
        if vbegin == vend {
            return;
        }
        let parm: PhyAddrRefMasked = self.root_pam().into();
        let l = &vbegin.indexes();
        let r = &vend.sub_one_page().indexes();
        Self::unmap_range_0(parm, l, r);
    }
    fn unmap_range_0(parm: PhyAddrRefMasked, l: &[usize; 3], r: &[usize; 3]) {
        let xbegin = &[0, 0];
        let xend = &[511, 511];
        for (i, pte) in &mut parm.as_pte_array_mut()[l[0]..=r[0]].iter_mut().enumerate() {
            let (l, r, full) = Self::next_lr(l, r, xbegin, xend, i);
            if full {
                // 1GB page table
                debug_check!(pte.is_leaf());
                *pte = PageTableEntry::empty();
            } else {
                debug_check!(pte.is_directory());
                Self::unmap_range_1(pte, l, r);
            }
        }
    }
    #[inline(always)]
    fn unmap_range_1(pte: &mut PageTableEntry, l: &[usize; 2], r: &[usize; 2]) {
        let xbegin = &[0];
        let xend = &[511];
        for (i, pte) in &mut PhyAddrRefMasked::from(pte.phy_addr()).as_pte_array_mut()[l[0]..=r[0]]
            .iter_mut()
            .enumerate()
        {
            let (l, r, full) = Self::next_lr(l, r, xbegin, xend, i);
            if full {
                debug_check!(pte.is_leaf());
                *pte = PageTableEntry::empty();
            } else {
                debug_check!(pte.is_directory());
                Self::unmap_range_2(pte, l, r);
            }
        }
    }
    #[inline(always)]
    fn unmap_range_2(pte: &mut PageTableEntry, l: &[usize; 1], r: &[usize; 1]) {
        for pte in &mut PhyAddrRefMasked::from(pte.phy_addr()).as_pte_array_mut()[l[0]..=r[0]] {
            debug_check!(pte.is_leaf());
            *pte = PageTableEntry::empty();
        }
    }
    /// if return Err, frame exhausted.
    pub fn map(
        &mut self,
        vam: VirAddrMasked,
        parm: PhyAddrRefMasked,
        flags: PTEFlags,
    ) -> Result<(), ()> {
        let pte = self.find_pte_create(vam)?;
        debug_check!(!pte.is_valid(), "vam {:?} is mapped before mapping", vam);
        *pte = PageTableEntry::new(parm.into(), flags | PTEFlags::V);
        Ok(())
    }
    pub fn unmap(&mut self, vam: VirAddrMasked) {
        let pte = self.find_pte(vam).expect("unmap invalid virtual address!");
        assert!(pte.is_valid(), "pte {:?} is invalid before unmapping", pte);
        *pte = PageTableEntry::empty();
    }
    pub fn translate(&self, vam: VirAddrMasked) -> Option<PageTableEntry> {
        self.find_pte(vam).map(|pte| *pte)
    }
    pub unsafe fn translate_uncheck(&self, vam: VirAddrMasked) -> PageTableEntry {
        *self
            .find_pte(vam)
            .unwrap_or_else(|| panic!("translate_uncheck: invalid pte from {:?}", vam))
    }
    pub fn copy_kernel_from(&mut self, src: &Self) {
        let src = src.root_pam().into_ref().as_pte_array();
        let dst = self.root_pam().into_ref().as_pte_array_mut();
        dst[0..256].copy_from_slice(&src[0..256]);
        // dst.array_chunks_mut::<256>();
    }
}

/// new a kernel page table
/// set asid to 0.
/// if return None, means no enough memory.
fn new_kernel_page_table() -> Result<PageTable, ()> {
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
    let mut page_table = PageTable::new_empty(0)?;
    fn get_usize_va(va: usize) -> VirAddrMasked {
        unsafe { VirAddrMasked::from_usize(va) }
    }
    fn get_usize_pa(pa: usize) -> PhyAddrRefMasked {
        unsafe { PhyAddrRefMasked::from_usize(pa) }
    }
    fn get_va(xva: usize) -> VirAddrMasked {
        get_usize_va(xva)
    }
    fn pa_from_kernel(xva: usize) -> PhyAddrRefMasked {
        get_usize_pa(xva - KERNEL_OFFSET_FROM_DIRECT_MAP)
    }
    fn pa_from_dirref(xva: usize) -> PhyAddrRefMasked {
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
        pa_fn: impl FnOnce(usize) -> PhyAddrRefMasked,
    ) {
        pt.map_range(get_va(b), pa_fn(b), get_size(b, e), flags)
            .unwrap();
    }
    fn xmap_impl_kernel(pt: &mut PageTable, b: usize, e: usize, flags: PTEFlags) {
        pt.map_range(get_va(b), pa_from_kernel(b), get_size(b, e), flags)
            .unwrap();
    }
    fn xmap_impl_dirref(pt: &mut PageTable, b: usize, e: usize, flags: PTEFlags) {
        pt.map_range(get_va(b), pa_from_dirref(b), get_size(b, e), flags)
            .unwrap();
    }
    fn xmap_kernel(
        pt: &mut PageTable,
        b: unsafe extern "C" fn(),
        e: unsafe extern "C" fn(),
        flags: PTEFlags,
    ) {
        xmap_impl_kernel(pt, b as usize, e as usize, flags);
    }
    let execable = PTEFlags::G | PTEFlags::R | PTEFlags::X;
    let readonly = PTEFlags::G | PTEFlags::R;
    let writable = PTEFlags::G | PTEFlags::R | PTEFlags::W;
    xmap_kernel(&mut page_table, stext, etext, execable);
    xmap_kernel(&mut page_table, srodata, erodata, readonly);
    xmap_kernel(&mut page_table, sdata, edata, writable);
    xmap_kernel(&mut page_table, sstack, estack, writable);
    xmap_kernel(&mut page_table, sbss, ebss, writable);
    // memory used in init frame.
    xmap_impl_kernel(&mut page_table, end as usize, INIT_MEMORY_END, writable);
    // direct map
    println!("map DIRECT_MAP");
    xmap_impl_dirref(&mut page_table, DIRECT_MAP_BEGIN, DIRECT_MAP_END, writable);
    Ok(page_table)
}

pub unsafe fn translated_byte_buffer_force(
    satp: usize,
    ptr: *const u8,
    len: usize,
) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_satp(satp);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirAddr::from(start);
        let mut vam = start_va.floor();
        let parm: PhyAddrRefMasked = page_table.translate_uncheck(vam).phy_addr().into(); // unsafe
        vam.step();
        let end_va = VirAddr::from(vam).min(VirAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut parm.as_bytes_array_mut()[start_va.page_offset()..]);
        } else {
            v.push(&mut parm.as_bytes_array_mut()[start_va.page_offset()..end_va.page_offset()]);
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
        let xptr = PhyAddrRefMasked::from_usize(ptr as usize - KERNEL_OFFSET_FROM_DIRECT_MAP);
        *xptr.get_mut() = 1234usize;
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
        csr::sfence_vma_all_global();
        debug_run!(direct_map_test());
    }
}

/// used by another hart
pub unsafe fn set_satp_by_global() {
    csr::set_satp(
        KERNEL_GLOBAL
            .as_ref()
            .expect("KERNEL_GLOBAL has not been initialized")
            .satp(),
    );
    csr::sfence_vma_all_global();
}

/// auto free root space.
pub struct UserPageTable {
    page_table: PageTable,
}

impl UserPageTable {
    pub fn from_global(asid: usize) -> Result<Self, ()> {
        Ok(Self {
            page_table: PageTable::from_global(asid)?,
        })
    }
    pub fn satp(&self) -> usize {
        self.page_table.satp()
    }
    pub fn check_user_empty(&self) {
        // if error happen, panic.
        todo!()
    }
}

impl Drop for UserPageTable {
    fn drop(&mut self) {
        debug_run! {self.check_user_empty()};
        let ptr = self.page_table.root_pam();
        unsafe {
            frame_dealloc_dpa(ptr);
        }
    }
}
