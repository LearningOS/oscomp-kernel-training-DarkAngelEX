use core::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    hart::csr,
    impl_usize_from, local,
    memory::{address::VirAddr, page_table::PageTable},
    sync::mutex::SpinNoIrqLock,
    tools::container::{never_clone_linked_list::NeverCloneLinkedList, Stack},
};

pub const USING_ASID: bool = false;

const ASID_BIT: usize = 16;
const MAX_ASID: usize = 1usize << ASID_BIT;
const ASID_MASK: usize = MAX_ASID - 1;
const ASID_VERSION_MASK: usize = !ASID_MASK;

/// raw asid, assume self & ASID_MASK == self
#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Asid(usize);
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
/// raw asid, assume self & ASID_VERSION_MASK == self
pub struct AsidVersion(usize);
/// self = AsidVersion | Asid
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct AsidInfo(usize);

pub struct AtomicAsidInfo(AtomicUsize);

pub struct AsidInfoTracker {
    asid_info: AtomicAsidInfo,
}
impl Drop for AsidInfoTracker {
    fn drop(&mut self) {
        unsafe { dealloc_asid(self.asid_info.get()) }
    }
}

impl AtomicAsidInfo {
    const ZERO: Self = AtomicAsidInfo(AtomicUsize::new(0));
    pub fn new(ai: AsidInfo) -> Self {
        Self(AtomicUsize::new(ai.into_usize()))
    }
    pub fn get(&self) -> AsidInfo {
        AsidInfo(self.0.load(Ordering::Relaxed))
    }
    fn set(&self, new: AsidInfo, _asid_manager: &mut AsidManager) {
        self.0.store(new.into_usize(), Ordering::Relaxed);
    }
}

impl AsidInfoTracker {
    const ZERO: Self = AsidInfoTracker {
        asid_info: AtomicAsidInfo::ZERO,
    };
    fn alloc() -> Self {
        alloc_asid()
    }
    fn build(version: AsidVersion, asid: Asid) -> Self {
        Self {
            asid_info: AtomicAsidInfo::new(AsidInfo(version.into_usize() | asid.into_usize())),
        }
    }
    pub fn consume(self) -> AsidInfo {
        let asid = self.asid_info.get();
        core::mem::forget(self);
        asid
    }
    pub fn asid(&self) -> Asid {
        self.asid_info.get().asid()
    }
}

impl Debug for AsidInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "id:{} version:{}",
            self.asid().into_usize(),
            self.version().into_usize()
        ))
    }
}

impl_usize_from!(Asid, v, v.0);
impl_usize_from!(AsidVersion, v, v.0);
impl_usize_from!(AsidInfo, v, v.0);

impl Asid {
    // const ZERO: Self = Asid(0);
    fn is_valid(&self) -> bool {
        self.into_usize() < MAX_ASID
    }
    fn increase(&mut self) {
        self.0 += 1;
    }
    fn reset(&mut self) {
        self.0 = 1;
    }
}

impl AsidInfo {
    pub fn new(version: AsidVersion, asid: Asid) -> Self {
        Self(version.into_usize() | asid.into_usize())
    }
    pub fn asid(&self) -> Asid {
        Asid(self.0 & ASID_MASK)
    }
    pub fn version(&self) -> AsidVersion {
        AsidVersion(self.0 & ASID_VERSION_MASK)
    }
}

impl AsidVersion {
    /// unique method to get unsafe AsidVersion
    pub const fn first_asid_version() -> Self {
        Self(0)
    }
    pub fn increase(&mut self) {
        self.0 += MAX_ASID;
    }
}

struct AsidManager {
    version: AsidVersion,
    current: Asid,
    recycled: NeverCloneLinkedList<Asid>,
}

impl AsidManager {
    pub const fn new() -> Self {
        Self {
            version: AsidVersion::first_asid_version(),
            current: Asid(1),
            recycled: NeverCloneLinkedList::new(),
        }
    }
    // this function is running in lock.
    pub fn version_check_alloc(&mut self, asid_info: &AsidInfoTracker, satp: &AtomicUsize) {
        let ai = asid_info.asid_info.get();
        local::asid_version_update(self.version);
        if ai.version() == self.version {
            return;
        }
        debug_assert!(ai.version() < self.version);
        let new_asid_info = self.alloc().consume();
        let new_asid = new_asid_info.asid();
        asid_info.asid_info.set(new_asid_info, self);
        let old_satp = satp.load(Ordering::Relaxed);
        let new_satp = PageTable::change_satp_asid(old_satp, new_asid.into_usize());
        satp.store(new_satp, Ordering::Relaxed);
    }
    pub fn alloc(&mut self) -> AsidInfoTracker {
        if let Some(asid) = self.recycled.pop() {
            return AsidInfoTracker::build(self.version, asid);
        }
        if self.current.is_valid() {
            let asid = self.current;
            self.current.increase();
            return AsidInfoTracker::build(self.version, asid);
        }
        // change version
        self.version.increase();
        self.recycled.clear();
        self.current.reset();
        let asid = self.current;
        self.current.increase();
        AsidInfoTracker::build(self.version, asid)
    }
    pub unsafe fn dealloc(&mut self, asid_info: AsidInfo) {
        if asid_info.version() == self.version {
            self.recycled.push(asid_info.asid());
        }
    }
}

static ASID_MANAGER: SpinNoIrqLock<AsidManager> = SpinNoIrqLock::new(AsidManager::new());

pub fn alloc_asid() -> AsidInfoTracker {
    if USING_ASID {
        ASID_MANAGER.lock().alloc()
    } else {
        AsidInfoTracker::ZERO
    }
}

pub unsafe fn dealloc_asid(asid_info: AsidInfo) {
    if USING_ASID {
        // 在这里调用降低锁竞争
        local::all_hart_sfence_vma_asid(asid_info.asid());
        ASID_MANAGER.lock().dealloc(asid_info)
    } else {
        local::all_hart_sfence_vma_all_no_global();
    }
}

pub fn version_check_alloc(asid_info: &AsidInfoTracker, satp: &AtomicUsize) {
    if USING_ASID {
        ASID_MANAGER.lock().version_check_alloc(asid_info, satp)
    }
}

pub fn asid_test() {
    use crate::memory::{address::VirAddr4K, allocator::frame, page_table::PTEFlags};

    fn va_set(va: VirAddr, value: usize) {
        unsafe { core::ptr::write_volatile(va.as_mut(), value) }
    }
    fn va_get(va: VirAddr) -> usize {
        unsafe { core::ptr::read_volatile(va.as_ref()) }
    }

    println!("[FTL OS]asid test");

    if !USING_ASID {
        println!("[FTL OS]don't use asid, skip test");
        return;
    }

    let mut space_1 = PageTable::from_global(AsidInfoTracker::alloc()).unwrap();
    let mut space_2 = PageTable::from_global(AsidInfoTracker::alloc()).unwrap();
    let va4k: VirAddr4K = unsafe { VirAddr4K::from_usize(0x1000) };
    let va: VirAddr = va4k.into();
    let pax1 = frame::global::alloc().unwrap();
    let pax2 = frame::global::alloc().unwrap();
    let pa1 = pax1.data();
    let pa2 = pax2.data();
    let flags = PTEFlags::R | PTEFlags::W;

    let allocator = &mut frame::defualt_allocator();

    space_1.map_par(va4k, pa1, flags, allocator).unwrap();
    space_2.map_par(va4k, pa2, flags, allocator).unwrap();
    let old_satp = unsafe { csr::get_satp() };

    unsafe {
        use crate::hart::sfence;

        println!("[FTL OS]asid test: flush all");
        space_1.using();
        sfence::sfence_vma_all_global();
        va_set(va, 1);
        space_2.using();
        sfence::sfence_vma_all_global();
        va_set(va, 2);
        space_1.using();
        sfence::sfence_vma_all_global();
        assert_eq!(va_get(va), 1);
        space_2.using();
        sfence::sfence_vma_all_global();
        assert_eq!(va_get(va), 2);
        sfence::sfence_vma_all_global();
        println!("[FTL OS]asid test: flush one");
        space_1.using();
        sfence::sfence_vma_va_global(va.into_usize());
        va_set(va, 3);
        space_2.using();
        sfence::sfence_vma_va_global(va.into_usize());
        va_set(va, 4);
        space_1.using();
        sfence::sfence_vma_va_global(va.into_usize());
        assert_eq!(va_get(va), 3);
        space_2.using();
        sfence::sfence_vma_va_global(va.into_usize());
        assert_eq!(va_get(va), 4);
        sfence::sfence_vma_all_global();
        // pnaic in hifive unmatched
        if false {
            println!("[FTL OS]asid test: flush error one");
            space_1.using();
            sfence::sfence_vma_va_global(0x80000);
            va_set(va, 5);
            space_2.using();
            sfence::sfence_vma_va_global(0x80000);
            va_set(va, 6);
            space_1.using();
            sfence::sfence_vma_va_global(0x80000);
            assert_eq!(va_get(va), 5);
            space_2.using();
            sfence::sfence_vma_va_global(0x80000);
            assert_eq!(va_get(va), 6);
            sfence::sfence_vma_all_global();
        }
        println!("[FTL OS]asid test: flush asid");

        let asid_1 = space_1.asid();
        let asid_2 = space_2.asid();
        println!("test case 1 {:?} {:?}", asid_1, asid_2);
        for i in 0..100 {
            space_1.using();
            va_set(va, 7);
            sfence::sfence_vma_asid(asid_1.into_usize());
            space_2.using();
            va_set(va, 8);
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_1.using();
            if va_get(va) != 7 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(asid_1.into_usize());
            space_2.using();
            if va_get(va) != 8 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(asid_2.into_usize());
        }
        sfence::sfence_vma_all_global();

        let asid_1 = space_1.asid();
        let asid_2 = space_2.asid();
        println!("test case 2 {:?} {:?}", asid_1, asid_2);
        for i in 0..100 {
            let v1 = i * 2 + 1;
            let v2 = i * 2 + 2;
            space_1.using();
            va_set(va, v1);
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_2.using();
            va_set(va, v2);
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_1.using();
            if va_get(va) != v1 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_2.using();
            if va_get(va) != v2 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
        }
        sfence::sfence_vma_all_global();

        let asid_1 = space_1.asid();
        let asid_2 = space_2.asid();
        println!("test case 3 {:?} {:?}", asid_1, asid_2);
        for i in 0..100 {
            let v1 = i * 2 + 1;
            let v2 = i * 2 + 2;
            space_1.using();
            va_set(va, v1);
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_2.using();
            va_set(va, v2);
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_1.using();
            if va_get(va) != v1 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
            space_2.using();
            if va_get(va) != v2 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(asid_1.into_usize());
            sfence::sfence_vma_asid(asid_2.into_usize());
        }
        sfence::sfence_vma_all_global();

        println!("[FTL OS]asid test: flush error asid");
        for i in 0..100 {
            let v1 = i * 2 + 1;
            let v2 = i * 2 + 2;
            space_1.using();
            va_set(va, v1);
            sfence::sfence_vma_asid(345);
            space_2.using();
            va_set(va, v2);
            sfence::sfence_vma_asid(345);
            space_1.using();
            if va_get(va) != v1 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(345);
            space_2.using();
            if va_get(va) != v2 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            sfence::sfence_vma_asid(345);
        }
        sfence::sfence_vma_all_global();

        println!("[FTL OS]asid test: just wait");
        for i in 0..100 {
            let v1 = i * 2 + 1;
            let v2 = i * 2 + 2;
            space_1.using();
            va_set(va, v1);
            for j in 0..100 {
                core::hint::black_box(j);
            }
            space_2.using();
            va_set(va, v2);
            for j in 0..100 {
                core::hint::black_box(j);
            }
            space_1.using();
            if va_get(va) != v1 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            for j in 0..100 {
                core::hint::black_box(j);
            }
            space_2.using();
            if va_get(va) != v2 {
                println!("error in iter:{} line: {}", i, line!());
                break;
            }
            for j in 0..100 {
                core::hint::black_box(j);
            }
        }
        sfence::sfence_vma_all_global();

        sfence::sfence_vma_all_global();
        csr::set_satp(old_satp);
        space_1.unmap_par(va4k, pa1);
        space_2.unmap_par(va4k, pa2);
    }
    println!("    asid test pass");
}
