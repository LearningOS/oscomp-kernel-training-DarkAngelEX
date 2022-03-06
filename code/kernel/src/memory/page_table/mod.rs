use core::{
    fmt::Debug,
    sync::atomic::{self, AtomicUsize, Ordering},
};

use alloc::vec::Vec;
use bitflags::bitflags;

use super::{
    address::{PhyAddr4K, PhyAddrRef4K, StepByOne, UserAddr4K, VirAddr, VirAddr4K},
    allocator::frame::{self, iter::FrameDataIter, FrameAllocator},
    asid::{self, AsidInfoTracker},
    user_space::UserArea,
};
use crate::{
    config::{
        DIRECT_MAP_BEGIN, DIRECT_MAP_END, INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP, PAGE_SIZE,
    },
    hart::{csr, sfence},
    tools::error::FrameOutOfMemory,
};

mod map_impl;

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

pub struct PageTable {
    asid_tracker: AsidInfoTracker,
    satp: AtomicUsize, // [63:60 MODE, 8 is SV39][59:44 ASID][43:0 PPN]
}

impl Drop for PageTable {
    fn drop(&mut self) {
        memory_trace!("PageTable::drop begin");
        assert!(self.satp() != 0);
        let cur_satp = unsafe { csr::get_satp() };
        assert_ne!(self.satp(), cur_satp);
        let allocator = &mut frame::defualt_allocator();
        self.free_user_directory_all(allocator);
        unsafe { allocator.dealloc(self.root_pa().into_ref()) };
        *self.satp.get_mut() = 0; // just for panic.
        memory_trace!("PageTable::drop end");
    }
}

impl PageTable {
    /// asid set to zero must be success.
    pub fn new_empty(asid_tracker: AsidInfoTracker) -> Result<Self, FrameOutOfMemory> {
        let phy_ptr = frame::global::alloc_dpa()?.consume();
        let arr = phy_ptr.into_ref().as_pte_array_mut();
        arr.iter_mut()
            .for_each(|pte| *pte = PageTableEntry::empty());
        let asid = asid_tracker.asid().into_usize();
        Ok(PageTable {
            asid_tracker,
            satp: AtomicUsize::new(8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn()),
        })
    }
    pub fn from_global(asid_tracker: AsidInfoTracker) -> Result<Self, FrameOutOfMemory> {
        memory_trace!("PageTable::from_global");
        let phy_ptr = frame::global::alloc_dpa()?.consume();
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
            satp: AtomicUsize::new(8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn()),
        })
    }
    fn satp(&self) -> usize {
        self.satp.load(Ordering::Relaxed)
    }
    /// used in AsidManager when update version.
    pub fn change_satp_asid(satp: usize, asid: usize) -> usize {
        (satp & !(0xffff << 44)) | (asid & 0xffff) << 44
    }
    unsafe fn set_satp_register_uncheck(&self) {
        csr::set_satp(self.satp())
    }
    pub unsafe fn using(&self) {
        self.version_check();
        self.set_satp_register_uncheck();
    }
    fn version_check(&self) {
        asid::version_check_alloc(&self.asid_tracker, &self.satp);
    }
    fn root_pa(&self) -> PhyAddr4K {
        PhyAddr4K::from_satp(self.satp())
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

    #[inline(always)]
    fn ptes_from_pte(pte: &mut PageTableEntry) -> &'static mut [PageTableEntry; 512] {
        debug_check!(pte.is_directory());
        PhyAddrRef4K::from(pte.phy_addr()).as_pte_array_mut()
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
        memory_trace!("PageTable::fork end");
        Ok(pt)
    }
}

/// new a kernel page table
/// set asid to 0.
/// if return None, means no enough memory.
fn new_kernel_page_table() -> Result<PageTable, FrameOutOfMemory> {
    extern "C" {
        // kernel segment ALIGN 4K
        fn stext();
        fn etext();
        // read only data segment ALIGN 4K
        fn srodata();
        fn erodata();
        // writable data segment ALIGN 4K
        fn sdata();
        // fn edata();
        // // stack ALIGN 4K
        // fn sstack();
        // fn estack();
        // // bss ALIGN 4K
        // fn sbss();
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
        atomic::fence(Ordering::Release);
        csr::set_satp(satp);
        sfence::sfence_vma_all_global();
        sfence::fence_i();
        debug_run!({ direct_map_test() });
    }
}

/// don't call sfence::fence_i();
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

#[derive(Debug)]
pub struct PageTableClosed;
