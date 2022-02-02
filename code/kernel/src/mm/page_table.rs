use core::fmt::Debug;

use alloc::vec::Vec;
use bitflags::bitflags;

use crate::{
    config::{INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP, PAGE_SIZE},
    debug::PRINT_MAP_ALL,
    mm::frame_allocator::frame_dealloc_dpa,
    riscv::csr,
};

use super::{
    address::{PhyAddr, PhyAddrMasked, PhyAddrRefMasked, StepByOne, VirAddr, VirAddrMasked},
    frame_allocator::{frame_alloc_dpa, FrameTrackerDpa},
};

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
    pub fn alloc(&mut self, flags: PTEFlags) -> Result<(), ()> {
        assert!(!self.is_valid(), "try alloc to a valid pte");
        let pam = frame_alloc_dpa()?.consume();
        PhyAddrRefMasked::from(pam)
            .get_pte_array_mut()
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

pub struct PageTable {
    satp: usize, // [63:60 MODE, 8 is SV39][59:44 ASID][43:0 PPN]
}

// /// Assume that it won't oom when creating/mapping.
impl PageTable {
    // asid set to zero must be success.
    pub fn new(asid: usize) -> Result<Self, ()> {
        let ppn = frame_alloc_dpa()?.consume().ppn();
        Ok(PageTable {
            satp: 8usize << 60 | (asid & 0xffff) << 44 | ppn,
        })
    }
    pub fn satp(&self) -> usize {
        self.satp
    }
    pub fn set_asid(&mut self, asid: usize) {
        self.satp = (self.satp & !(0xffff << 44)) | (asid & 0xffff) << 44
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_satp(satp: usize) -> Self {
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
            let pte = &mut parm.get_pte_array_mut()[idx];
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
            let pte = &mut parm.get_pte_array_mut()[idx];
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
    /// PhyAddrRefMasked
    ///
    /// assume memory enough
    pub fn map_range_with_direct(
        &mut self,
        vbegin: VirAddrMasked,
        pbegin: PhyAddrRefMasked,
        size: usize,
        flags: PTEFlags,
    ) -> &mut Self {
        self.map_range(vbegin, pbegin, size, flags).unwrap();
        let xvbegin = unsafe { VirAddrMasked::from_usize(usize::from(pbegin)) };
        self.map_range(xvbegin, pbegin, size, flags).unwrap()
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
        let [x0_l, x1_l, x2_l] = vbegin.indexes();
        let [x0_r, x1_r, x2_r] = vend.sub_one_page().indexes();
        if PRINT_MAP_ALL {
            println!(
                "map_range: {:#x} - {:#x} size = {}",
                usize::from(vbegin),
                usize::from(vend),
                size
            );
            println!("l:[{}, {}, {}]", x0_l, x1_l, x2_l);
            println!("r:[{}, {}, {}]", x0_r, x1_r, x2_r);
        }
        // clear 12 + 9 * 3 = 39 bit
        Self::alloc_range_0(
            parm,
            x0_l,
            x0_r,
            x1_l,
            x1_r,
            x2_l,
            x2_r,
            flags,
            vbegin,
            pbegin.into(),
        )?;
        Ok(self)
    }
    #[inline(always)]
    fn alloc_range_0(
        mut parm: PhyAddrRefMasked,
        x0_l: usize,
        x0_r: usize,
        x1_l: usize,
        x1_r: usize,
        x2_l: usize,
        x2_r: usize,
        flags: PTEFlags,
        mut va: VirAddrMasked,
        mut pa: PhyAddrMasked,
    ) -> Result<(), ()> {
        // println!("level 0: {:?} {}-{}", va, x0_l, x0_r);
        for (i, pte) in &mut parm.get_pte_array_mut()[x0_l..=x0_r].iter_mut().enumerate() {
            if !pte.is_valid() {
                pte.alloc(PTEFlags::V)?;
            }
            let (x1_l, x2_l) = if i == 0 { (x1_l, x2_l) } else { (0, 0) };
            let (x1_r, x2_r) = if i == x0_r - x0_l {
                (x1_r, x2_r)
            } else {
                (511, 511)
            };
            (va, pa) = Self::alloc_range_1(pte, x1_l, x1_r, x2_l, x2_r, flags, va, pa)?
        }
        Ok(())
    }
    #[inline(always)]
    fn alloc_range_1(
        pte: &mut PageTableEntry,
        x1_l: usize,
        x1_r: usize,
        x2_l: usize,
        x2_r: usize,
        flags: PTEFlags,
        mut va: VirAddrMasked,
        mut pa: PhyAddrMasked,
    ) -> Result<(VirAddrMasked, PhyAddrMasked), ()> {
        // println!("level 1: {:?} {}-{}", va, x1_l, x1_r);
        for (i, pte) in &mut PhyAddrRefMasked::from(pte.phy_addr()).get_pte_array_mut()[x1_l..=x1_r]
            .iter_mut()
            .enumerate()
        {
            if !pte.is_valid() {
                pte.alloc(PTEFlags::V)?;
            }
            let x2_l = if i == 0 { x2_l } else { 0 };
            let x2_r = if i == x1_r - x1_l { x2_r } else { 511 };
            (va, pa) = Self::alloc_range_2(pte, x2_l, x2_r, flags, va, pa);
        }
        Ok((va, pa))
    }
    #[inline(always)]
    fn alloc_range_2(
        pte: &mut PageTableEntry,
        x2_l: usize,
        x2_r: usize,
        flags: PTEFlags,
        mut va: VirAddrMasked,
        mut pa: PhyAddrMasked,
    ) -> (VirAddrMasked, PhyAddrMasked) {
        // println!("level 2: {:?}", va);
        for pte in &mut PhyAddrRefMasked::from(pte.phy_addr()).get_pte_array_mut()[x2_l..=x2_r] {
            assert!(!pte.is_valid(), "remap of {:?} -> {:?}", va, pa);
            // if PRINT_MAP_ALL {
            //     println!("{:?} -> {:?}", va, pa);
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
    #[allow(unused)]
    pub fn unmap_range(&mut self, vbegin: VirAddrMasked, vend: VirAddrMasked) {
        assert!(vbegin <= vend, "free_range vbegin <= vend");
        if vbegin == vend {
            return;
        }
        let parm: PhyAddrRefMasked = self.root_pam().into();
        let [x0_l, x1_l, x2_l] = vbegin.indexes();
        let [x0_r, x1_r, x2_r] = vend.sub_one_page().indexes();
        Self::free_range_0(parm, x0_l, x0_r, x1_l, x1_r, x2_l, x2_r);
    }
    #[inline(always)]
    fn free_range_0(
        mut parm: PhyAddrRefMasked,
        x0_l: usize,
        x0_r: usize,
        x1_l: usize,
        x1_r: usize,
        x2_l: usize,
        x2_r: usize,
    ) {
        for (i, pte) in &mut parm.get_pte_array_mut()[x0_l..=x0_r].iter_mut().enumerate() {
            if pte.is_valid() {
                let (x1_l, x2_l) = if i == 0 { (x1_l, x2_l) } else { (0, 0) };
                let (x1_r, x2_r) = if i == x0_r - x0_l {
                    (x1_r, x2_r)
                } else {
                    (511, 511)
                };
                Self::free_range_1(pte, x1_l, x1_r, x2_l, x2_r);
            }
        }
    }
    #[inline(always)]
    fn free_range_1(pte: &mut PageTableEntry, x1_l: usize, x1_r: usize, x2_l: usize, x2_r: usize) {
        for (i, pte) in &mut PhyAddrRefMasked::from(pte.phy_addr()).get_pte_array_mut()[x1_l..=x1_r]
            .iter_mut()
            .enumerate()
        {
            if pte.is_valid() {
                let x2_l = if i == 0 { x2_l } else { 0 };
                let x2_r = if i == x1_r - x1_l { x2_r } else { 511 };
                Self::free_range_2(pte, x2_l, x2_r);
                if x2_l == 0 && x2_r == 511 {
                    unsafe { pte.dealloc() }
                }
            }
        }
    }
    #[inline(always)]
    fn free_range_2(pte: &mut PageTableEntry, x2_l: usize, x2_r: usize) {
        for pte in &mut PhyAddrRefMasked::from(pte.phy_addr()).get_pte_array_mut()[x2_l..=x2_r] {
            if pte.is_valid() {
                unsafe { pte.dealloc() }
            }
        }
    }
    /// if return Err, frame exhausted.
    #[allow(unused)]
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
    #[allow(unused)]
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
        let src = src.root_pam().into_ref().get_pte_array();
        let dst = self.root_pam().into_ref().get_pte_array_mut();
        dst[0..256].copy_from_slice(&src[0..256]);
        // dst.array_chunks_mut::<256>();
    }
}

/// new a kernel page table
/// set asid to 0.
/// if return None, means no enough memory.
pub fn new_kernel_page_table() -> Result<PageTable, ()> {
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
    let mut page_table = PageTable::new(0)?;
    fn get_usize_va(va: usize) -> VirAddrMasked {
        unsafe { VirAddrMasked::from_usize(va) }
    }
    fn get_usize_pa(pa: usize) -> PhyAddrRefMasked {
        unsafe { PhyAddrRefMasked::from_usize(pa) }
    }
    fn get_va(xva: usize) -> VirAddrMasked {
        get_usize_va(xva)
    }
    fn get_pa(xva: usize) -> PhyAddrRefMasked {
        get_usize_pa(xva - KERNEL_OFFSET_FROM_DIRECT_MAP)
    }
    fn get_size(b: usize, e: usize) -> usize {
        assert!(b % PAGE_SIZE == 0);
        assert!(e % PAGE_SIZE == 0);
        e.checked_sub(b).unwrap()
    }
    fn xmap_impl(pt: &mut PageTable, b: usize, e: usize, flags: PTEFlags) {
        pt.map_range_with_direct(get_va(b), get_pa(b), get_size(b, e), flags);
    }
    fn xmap(
        pt: &mut PageTable,
        b: unsafe extern "C" fn(),
        e: unsafe extern "C" fn(),
        flags: PTEFlags,
    ) {
        xmap_impl(pt, b as usize, e as usize, flags);
    }
    let execable = PTEFlags::G | PTEFlags::R | PTEFlags::X;
    let readonly = PTEFlags::G | PTEFlags::R;
    let writable = PTEFlags::G | PTEFlags::R | PTEFlags::W;
    xmap(&mut page_table, stext, etext, execable);
    xmap(&mut page_table, srodata, erodata, readonly);
    xmap(&mut page_table, sdata, edata, writable);
    xmap(&mut page_table, sstack, estack, writable);
    xmap(&mut page_table, sbss, ebss, writable);
    // memory used in init frame.
    xmap_impl(&mut page_table, end as usize, INIT_MEMORY_END, writable);
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
        let mut parm: PhyAddrRefMasked = page_table.translate_uncheck(vam).phy_addr().into(); // unsafe
        vam.step();
        let end_va = VirAddr::from(vam).min(VirAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut parm.get_bytes_array_mut()[start_va.page_offset()..]);
        } else {
            v.push(&mut parm.get_bytes_array_mut()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

pub fn init_kernel_page_table() {
    print!("[FTL OS]init kerne page table");
    let page_table = new_kernel_page_table().expect("new kernel page table error.");
    unsafe {
        csr::set_satp(page_table.satp());
        csr::sfence_vma_all_global();
    }
}
