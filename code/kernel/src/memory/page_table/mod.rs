use core::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::vec::Vec;

use super::{
    address::{PhyAddr4K, PhyAddrRef4K, StepByOne, UserAddr4K, VirAddr, VirAddr4K},
    allocator::frame::{self, iter::FrameDataIter, FrameAllocator},
    asid::{self, Asid, AsidInfoTracker},
    user_space::UserArea,
};
use crate::{
    config::{
        DIRECT_MAP_BEGIN, DIRECT_MAP_END, INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP, PAGE_SIZE,
    },
    hart::{csr, sfence},
    local,
    memory::address::PhyAddrRef,
    tools::{error::FrameOOM, DynDropRun},
};

mod map_impl;
pub mod pte_iter;

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
    }
}

impl PTEFlags {
    pub fn writable(self) -> bool {
        self.contains(Self::W)
    }
    pub fn executable(self) -> bool {
        self.contains(Self::X)
    }
}

const PTE_SHARED: usize = 1 << 8;

#[derive(Copy, Clone)]
#[repr(C)]
pub struct PageTableEntry {
    bits: usize,
}

impl PageTableEntry {
    pub fn new(pa_4k: PhyAddr4K, perm: PTEFlags) -> Self {
        PageTableEntry {
            bits: (usize::from(pa_4k) >> 2) & ((1 << 54usize) - 1) | perm.bits as usize,
        }
    }
    pub const EMPTY: Self = Self { bits: 0 };
    pub fn reset(&mut self) {
        *self = Self::EMPTY;
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
    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    pub fn is_directory(&self) -> bool {
        let mask = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U;
        self.is_valid() && (self.flags() & mask) == PTEFlags::empty()
    }
    pub fn is_leaf(&self) -> bool {
        let mask = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U;
        self.is_valid() && (self.flags() & mask) != PTEFlags::empty()
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
    pub fn shared(&self) -> bool {
        self.bits & PTE_SHARED != 0
    }
    pub fn rsw_8(&self) -> bool {
        (self.bits & 1usize << 8) != 0
    }
    pub fn rsw_9(&self) -> bool {
        (self.bits & 1usize << 9) != 0
    }
    pub fn reserved_bit(&self) -> usize {
        self.bits & (((1usize << 10) - 1) << 54)
    }
    pub fn set_rwx(&mut self, flag: PTEFlags) {
        let mask = (PTEFlags::R | PTEFlags::W | PTEFlags::X).bits() as usize;
        let flag = flag.bits() as usize & mask;
        self.bits = (self.bits & !mask) | flag;
    }
    pub fn set_writable(&mut self) {
        self.bits |= PTEFlags::W.bits() as usize;
    }
    pub fn clear_writable(&mut self) {
        self.bits &= !(PTEFlags::W.bits() as usize);
    }
    pub fn set_shared(&mut self) {
        self.bits |= PTE_SHARED;
    }
    pub fn clear_shared(&mut self) {
        self.bits &= !PTE_SHARED;
    }
    pub fn become_shared(&mut self, shared_writable: bool) {
        debug_assert!(!self.shared());
        self.set_shared();
        if !shared_writable {
            self.clear_writable();
        }
    }
    pub fn become_unique(&mut self, unique_writable: bool) {
        debug_assert!(self.shared());
        self.clear_shared();
        if unique_writable {
            self.set_writable();
        }
    }
    /// 用来分配非叶节点, 不能包含URW标志位
    pub fn alloc_by_non_leaf(
        &mut self,
        perm: PTEFlags,
        allocator: &mut dyn FrameAllocator,
    ) -> Result<(), FrameOOM> {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        debug_assert!(!perm.intersects(PTEFlags::U | PTEFlags::R | PTEFlags::W));
        let pa = allocator.alloc_directory()?.consume();
        *self = Self::new(PhyAddr4K::from(pa), perm | PTEFlags::V);
        Ok(())
    }

    /// 为这个页节点分配实际物理页, 不会填充任何数据! 需要手动初始化内存
    pub fn alloc_by(
        &mut self,
        perm: PTEFlags,
        allocator: &mut dyn FrameAllocator,
    ) -> Result<(), FrameOOM> {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        let pa = allocator.alloc()?.consume();
        *self = Self::new(
            PhyAddr4K::from(pa),
            perm | PTEFlags::D | PTEFlags::A | PTEFlags::V,
        );
        Ok(())
    }
    pub fn alloc_by_frame(&mut self, perm: PTEFlags, pa: PhyAddrRef4K) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        *self = Self::new(
            PhyAddr4K::from(pa),
            perm | PTEFlags::D | PTEFlags::A | PTEFlags::V,
        );
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc_by(&mut self, allocator: &mut dyn FrameAllocator) {
        debug_assert!(self.is_valid() && self.is_leaf());
        allocator.dealloc(self.phy_addr().into());
        *self = Self::EMPTY;
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc_by_non_leaf(&mut self, allocator: &mut dyn FrameAllocator) {
        debug_assert!(self.is_valid() && self.is_directory());
        allocator.dealloc_directory(self.phy_addr().into());
        *self = Self::EMPTY;
    }
}

impl Debug for PageTableEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("PTE:{:#x}", self.bits))
    }
}

pub struct PageTable {
    // modity asid_tracker and satp only in version_check
    asid_tracker: AsidInfoTracker,
    satp: AtomicUsize, // [63:60 MODE, 8 is SV39][59:44 ASID][43:0 PPN]
}

impl Drop for PageTable {
    fn drop(&mut self) {
        memory_trace!("PageTable::drop begin");
        assert!(self.satp() != 0);
        let cur_satp = unsafe { csr::get_satp() };
        assert_ne!(self.satp(), cur_satp);
        let allocator = &mut frame::default_allocator();
        self.free_user_directory_all(allocator);
        unsafe { allocator.dealloc(self.root_pa().into_ref()) };
        *self.satp.get_mut() = 0; // just for panic.
        memory_trace!("PageTable::drop end");
    }
}

impl PageTable {
    /// asid set to zero must be success.
    pub fn new_empty(asid_tracker: AsidInfoTracker) -> Result<Self, FrameOOM> {
        let phy_ptr = frame::global::alloc()?.consume();
        let arr = phy_ptr.as_pte_array_mut();
        arr.iter_mut().for_each(|pte| *pte = PageTableEntry::EMPTY);
        let asid = asid_tracker.asid().into_usize();
        let phy_ptr: PhyAddr4K = phy_ptr.into();
        Ok(PageTable {
            asid_tracker,
            satp: AtomicUsize::new(8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn()),
        })
    }
    pub fn from_global(asid_tracker: AsidInfoTracker) -> Result<Self, FrameOOM> {
        memory_trace!("PageTable::from_global");
        let phy_ptr = frame::global::alloc()?.consume();
        unsafe {
            KERNEL_GLOBAL
                .as_ref()
                .unwrap()
                .write_kernel_init_to(phy_ptr.as_pte_array_mut());
        }
        let asid = asid_tracker.asid().into_usize();
        let phy_ptr: PhyAddr4K = phy_ptr.into();
        Ok(PageTable {
            asid_tracker,
            satp: AtomicUsize::new(8usize << 60 | (asid & 0xffff) << 44 | phy_ptr.ppn()),
        })
    }
    pub fn write_kernel_init_to(&self, dst: &mut [PageTableEntry; 512]) {
        let src = self.root_pa().into_ref().as_pte_array();
        dst[256..].copy_from_slice(&src[256..]);
        dst[..256].iter_mut().for_each(|pte| pte.reset());
    }
    fn satp(&self) -> usize {
        self.satp.load(Ordering::Relaxed)
    }
    pub fn in_using(&self) -> bool {
        let satp = unsafe { csr::get_satp() };
        satp == self.satp()
    }
    /// used in AsidManager when update version.
    pub(super) fn change_satp_asid(satp: usize, asid: usize) -> usize {
        (satp & !(0xffff << 44)) | (asid & 0xffff) << 44
    }
    pub fn asid(&self) -> Asid {
        self.asid_tracker.asid()
    }
    unsafe fn set_satp_register_uncheck(&self) {
        csr::set_satp(self.satp())
    }
    pub unsafe fn using(&self) {
        self.version_check();
        self.set_satp_register_uncheck();
    }
    /// 返回值析构时将刷表
    pub fn flush_asid_fn(&self) -> DynDropRun<Asid> {
        DynDropRun::new(self.asid(), local::all_hart_sfence_vma_asid)
    }
    /// 返回值析构时将刷表
    pub fn flush_va_asid_fn(&self, va: UserAddr4K) -> DynDropRun<(UserAddr4K, Asid)> {
        DynDropRun::new((va, self.asid()), |(va, asid)| {
            local::all_hart_sfence_vma_va_asid(va, asid)
        })
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
        allocator: &mut dyn FrameAllocator,
    ) -> Result<&mut PageTableEntry, FrameOOM> {
        let idxs = va.indexes();
        let mut par: PhyAddrRef4K = self.root_pa().into();
        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut par.as_pte_array_mut()[idx];
            if i == 2 {
                return Ok(pte);
            }
            if !pte.is_valid() {
                pte.alloc_by_non_leaf(PTEFlags::V, allocator)?;
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
        debug_assert!(pte.is_directory());
        PhyAddrRef4K::from(pte.phy_addr()).as_pte_array_mut()
    }

    /// if return Err, frame exhausted.
    pub fn map_par(
        &mut self,
        va: VirAddr4K,
        par: PhyAddrRef4K,
        flags: PTEFlags,
        allocator: &mut dyn FrameAllocator,
    ) -> Result<(), FrameOOM> {
        let pte = self.find_pte_create(va, allocator)?;
        debug_assert!(!pte.is_valid(), "va {:?} is mapped before mapping", va);
        *pte = PageTableEntry::new(par.into(), flags | PTEFlags::D | PTEFlags::A | PTEFlags::V);
        Ok(())
    }
    /// don't release space of par.
    pub fn unmap_par(&mut self, va: VirAddr4K, par: PhyAddrRef4K) {
        let pte = self.find_pte(va).expect("unmap invalid virtual address!");
        assert!(pte.is_valid(), "pte {:?} is invalid before unmapping", pte);
        assert!(pte.phy_addr().into_ref() == par);
        *pte = PageTableEntry::EMPTY;
    }
    pub fn translate(&self, va: VirAddr4K) -> Option<PageTableEntry> {
        self.find_pte(va).map(|pte| *pte)
    }
}

/// new a kernel page table
/// set asid to 0.
/// if return None, means no enough memory.
fn new_kernel_page_table() -> Result<PageTable, FrameOOM> {
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
    fn xmap_impl_kernel(
        pt: &mut PageTable,
        b: usize,
        e: usize,
        flags: PTEFlags,
        allocator: &mut dyn FrameAllocator,
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
        allocator: &mut dyn FrameAllocator,
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
        allocator: &mut dyn FrameAllocator,
    ) {
        xmap_impl_kernel(pt, b as usize, e as usize, flags, allocator);
    }
    let executable = PTEFlags::G | PTEFlags::R | PTEFlags::X;
    let readonly = PTEFlags::G | PTEFlags::R;
    let writable = PTEFlags::G | PTEFlags::R | PTEFlags::W;
    let allocator = &mut frame::default_allocator();
    xmap_kernel(&mut page_table, stext, etext, executable, allocator);
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

#[allow(dead_code)]
pub unsafe fn translated_byte_buffer_force(
    page_table: &PageTable,
    ptr: *const u8,
    len: usize,
) -> Vec<&'static mut [u8]> {
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirAddr::<u8>::from(start);
        let mut va4k = start_va.floor();
        let par: PhyAddrRef4K = page_table.translate(va4k).unwrap().phy_addr().into(); // unsafe
        va4k.step();
        let end_va = VirAddr::<u8>::from(va4k).min(VirAddr::from(end));
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
        let xptr = PhyAddrRef::from(ptr as usize - KERNEL_OFFSET_FROM_DIRECT_MAP);
        *xptr.get_mut() = 1234usize;
        assert_eq!(ptr.read_volatile(), 1234);
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
        sfence::fence_i();
        if cfg!(debug_assertions) {
            direct_map_test();
        }
    }
}

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
