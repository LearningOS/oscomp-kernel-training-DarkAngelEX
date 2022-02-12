use core::{fmt::Debug, ops::Deref};

use alloc::vec::Vec;

use crate::{
    impl_usize_from,
    memory::{address::VirAddr, page_table::PageTable},
    riscv::sfence,
    sync::mutex::SpinLock,
    tools,
};

const ASID_BIT: usize = 16;
const MAX_ASID: usize = 1usize << ASID_BIT;
const ASID_MASK: usize = MAX_ASID - 1;
const ASID_VERSION_MASK: usize = !ASID_MASK;

const TLB_SHOT_DOWM_IPML: bool = false;

/// raw asid, assume self & ASID_MASK == self
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Asid(usize);
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
/// raw asid, assume self & ASID_VERSION_MASK == self
pub struct AsidVersion(usize);
/// self = AsidVersion | Asid
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct AsidInfo(usize);

#[derive(Debug)]
pub struct AsidInfoTracker {
    asid_info: AsidInfo,
}
impl Drop for AsidInfoTracker {
    fn drop(&mut self) {
        unsafe { dealloc_asid(self.asid_info) }
    }
}
impl AsidInfoTracker {
    fn alloc() -> Self {
        alloc_asid()
    }
    fn build(version: AsidVersion, asid: Asid) -> Self {
        Self {
            asid_info: AsidInfo(version.into_usize() | asid.into_usize()),
        }
    }
    pub fn version_check(&mut self) -> Result<(), Asid> {
        match version_check_alloc(self.asid_info) {
            Ok(_null) => Ok(()),
            Err(new) => {
                self.asid_info = new.asid_info;
                core::mem::forget(new);
                Err(self.asid())
            }
        }
    }
}
impl Deref for AsidInfoTracker {
    type Target = AsidInfo;

    fn deref(&self) -> &Self::Target {
        &self.asid_info
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
    fn is_valid(&self) -> bool {
        self.into_usize() < MAX_ASID
    }
    fn increase(&mut self) {
        self.0 += 1;
    }
    fn reset(&mut self) {
        self.0 = 0;
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
    pub fn increase(&mut self) {
        self.0 += MAX_ASID;
    }
}

struct AsidManager {
    version: AsidVersion,
    current: Asid,
    recycled: Vec<Asid>,
}

impl AsidManager {
    pub const fn new() -> Self {
        Self {
            version: AsidVersion(0),
            current: Asid(0),
            recycled: Vec::new(),
        }
    }
    pub fn version_check(&self, asid_info: AsidInfo) -> bool {
        asid_info.version() == self.version
    }
    pub fn version_check_alloc(&mut self, asid_info: AsidInfo) -> Result<(), AsidInfoTracker> {
        tools::bool_result(asid_info.version() == self.version).map_err(|_| self.alloc())
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
            if TLB_SHOT_DOWM_IPML {
                todo!("TLB shot down"); // need TLB_SHOT_DOWN other hart.
                sfence::sfence_vma_asid(asid_info.asid().into_usize());
                self.recycled.push(asid_info.asid())
            }
        }
    }
}

static ASID_MANAGER: SpinLock<AsidManager> = SpinLock::new(AsidManager::new());

pub fn alloc_asid() -> AsidInfoTracker {
    ASID_MANAGER.lock().alloc()
}

pub unsafe fn dealloc_asid(asid_info: AsidInfo) {
    ASID_MANAGER.lock().dealloc(asid_info)
}

pub fn version_check(asid_info: AsidInfo) -> bool {
    ASID_MANAGER.lock().version_check(asid_info)
}

pub fn version_check_alloc(asid_info: AsidInfo) -> Result<(), AsidInfoTracker> {
    ASID_MANAGER.lock().version_check_alloc(asid_info)
}

// #[allow(dead_code)]
pub fn asid_test() {
    use crate::memory::{address::VirAddr4K, allocator::frame, page_table::PTEFlags};
    use crate::riscv::register::csr;

    fn va_set(va: VirAddr, value: usize) {
        unsafe {
            *va.as_mut() = value;
        }
    }
    fn va_get(va: VirAddr) -> usize {
        unsafe { *va.as_ref() }
    }

    println!("[FTL OS]asid test.");
    let mut space_1 = PageTable::from_global(AsidInfoTracker::alloc()).unwrap();
    let mut space_2 = PageTable::from_global(AsidInfoTracker::alloc()).unwrap();
    let va4k: VirAddr4K = unsafe { VirAddr4K::from_usize(0x1000) };
    let va: VirAddr = va4k.into();
    let pax1 = frame::alloc().unwrap();
    let pax2 = frame::alloc().unwrap();
    let pa1 = pax1.data();
    let pa2 = pax2.data();
    let flags = PTEFlags::R | PTEFlags::W;

    let allocator = &mut frame::defualt_allocator();

    space_1.map_par(va4k, pa1, flags, allocator).unwrap();
    space_2.map_par(va4k, pa2, flags, allocator).unwrap();
    let old_satp = unsafe { csr::get_satp() };

    space_1.using();
    va_set(va, 1);
    space_2.using();
    va_set(va, 2);
    space_1.using();
    assert_eq!(va_get(va), 1);
    space_2.using();
    assert_eq!(va_get(va), 2);

    unsafe { csr::set_satp(old_satp) };
    space_1.unmap_par(va4k, pa1);
    space_2.unmap_par(va4k, pa2);
}
