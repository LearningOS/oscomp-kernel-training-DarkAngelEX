//! Implementation of global allocator
//!
use crate::{
    config::{DIRECT_MAP_BEGIN, DIRECT_MAP_END, PAGE_SIZE},
    debug::trace::{self, OPEN_MEMORY_TRACE, TRACE_ADDR},
    tools::{allocator::Own, error::FrameOutOfMemory},
};
use alloc::vec::Vec;
///
/// this module will alloc frame(4KB)
use core::fmt::Debug;

use crate::{
    config::{INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP},
    memory::address::PhyAddrRef,
    sync::mutex::SpinLock,
};

use crate::memory::address::{PhyAddr4K, PhyAddrRef4K, StepByOne};

#[derive(Debug)]
pub struct FrameTracker {
    data: PhyAddrRef4K,
}
impl Own<PhyAddrRef4K> for FrameTracker {}
impl FrameTracker {
    pub unsafe fn new(data: PhyAddrRef4K) -> Self {
        Self { data }
    }
    pub fn data(&self) -> PhyAddrRef4K {
        self.data
    }
    pub fn consume(self) -> PhyAddrRef4K {
        let data = self.data;
        core::mem::forget(self);
        data
    }
    pub fn ptr(&self) -> PhyAddrRef4K {
        self.data
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        unsafe { dealloc(self.data) };
    }
}

pub struct FrameTrackerDpa {
    data: PhyAddr4K,
}
impl Own<PhyAddr4K> for FrameTrackerDpa {}

impl FrameTrackerDpa {
    pub unsafe fn new(data: PhyAddr4K) -> Self {
        Self { data }
    }
    pub fn data(&self) -> PhyAddr4K {
        self.data
    }
    pub fn consume(self) -> PhyAddr4K {
        let data = self.data;
        core::mem::forget(self);
        data
    }
}

impl Drop for FrameTrackerDpa {
    fn drop(&mut self) {
        unsafe { dealloc_dpa(self.data) };
    }
}

trait GlobalFrameAllocator {
    /// return count of frame, neither space size.
    fn size(&self) -> usize;
    fn alloc(&mut self) -> Result<PhyAddrRef4K, FrameOutOfMemory>;
    fn dealloc(&mut self, data: PhyAddrRef4K);
    fn alloc_range(&mut self, range: &mut [PhyAddrRef4K]) -> Result<(), FrameOutOfMemory>;
    fn dealloc_range(&mut self, range: &[PhyAddrRef4K]);
    fn alloc_dpa(&mut self) -> Result<PhyAddr4K, FrameOutOfMemory>;
    fn dealloc_dpa(&mut self, data: PhyAddr4K);
    fn alloc_range_dpa(&mut self, range: &mut [PhyAddr4K]) -> Result<(), FrameOutOfMemory>;
    fn dealloc_range_dpa(&mut self, range: &[PhyAddr4K]);
}

struct StackGlobalFrameAllocator {
    begin: PhyAddrRef4K, // used in recycle check.
    current: PhyAddrRef4K,
    end: PhyAddrRef4K,
    recycled: Vec<PhyAddrRef4K>,
}

impl StackGlobalFrameAllocator {
    const fn new() -> Self {
        Self {
            begin: unsafe { PhyAddrRef4K::from_usize(0) },
            current: unsafe { PhyAddrRef4K::from_usize(0) },
            end: unsafe { PhyAddrRef4K::from_usize(0) },
            recycled: Vec::new(),
        }
    }
    pub fn init(&mut self, begin: PhyAddrRef4K, end: PhyAddrRef4K) {
        assert!(begin < end);
        self.begin = begin;
        self.current = begin;
        self.end = end;
        println!(
            "StackFrameAllocator init range: [{:#x} - {:#x}]",
            usize::from(begin),
            usize::from(end)
        );
    }
    fn alloc_range_impl<T>(
        &mut self,
        range: &mut [T],
        f_tran: impl Fn(PhyAddrRef4K) -> T,
    ) -> Result<(), FrameOutOfMemory> {
        let n = range.len();
        let n0 = (usize::from(self.end) - usize::from(self.current)) / PAGE_SIZE;
        if n0 + self.recycled.len() < n {
            return Err(FrameOutOfMemory);
        }
        let nt = n.min(n0);
        for (i, pa) in range.iter_mut().take(nt).enumerate() {
            *pa = f_tran(self.current.add_one_page());
        }
        self.current = self.current.add_one_page();
        if n == nt {
            return Ok(());
        }
        for i in 0..n - nt {
            range[nt + i] = f_tran(self.recycled.pop().unwrap());
        }
        Ok(())
    }
}
impl GlobalFrameAllocator for StackGlobalFrameAllocator {
    fn size(&self) -> usize {
        self.recycled.len() + (self.end.into_usize() - self.current.into_usize()) / PAGE_SIZE
    }
    fn alloc(&mut self) -> Result<PhyAddrRef4K, FrameOutOfMemory> {
        fn pa_check(pa: PhyAddrRef4K) {
            assert!(pa.into_usize() > DIRECT_MAP_BEGIN && pa.into_usize() < DIRECT_MAP_END);
        }
        if let Some(pa) = self.recycled.pop() {
            pa_check(pa);
            Ok(pa)
        } else if self.current == self.end {
            Err(FrameOutOfMemory)
        } else {
            let ret = self.current;
            pa_check(ret);
            self.current.step();
            if OPEN_MEMORY_TRACE && ret == PhyAddrRef::from(TRACE_ADDR).floor() {
                trace::call_when_alloc();
            }
            Ok(ret)
        }
    }

    fn alloc_range(&mut self, range: &mut [PhyAddrRef4K]) -> Result<(), FrameOutOfMemory> {
        self.alloc_range_impl(range, |a| a)
    }

    fn dealloc(&mut self, data: PhyAddrRef4K) {
        if OPEN_MEMORY_TRACE && data == PhyAddrRef::from(TRACE_ADDR).floor() {
            trace::call_when_dealloc();
        }
        // return;
        // skip null
        debug_check!(data.into_usize() % PAGE_SIZE == 0);
        assert!(data.into_usize() > DIRECT_MAP_BEGIN && data.into_usize() < DIRECT_MAP_END);
        self.recycled.push(data);
    }

    fn dealloc_range(&mut self, range: &[PhyAddrRef4K]) {
        range.iter().for_each(|&a| self.recycled.push(a));
    }

    fn alloc_dpa(&mut self) -> Result<PhyAddr4K, FrameOutOfMemory> {
        self.alloc().map(|a| a.into())
    }

    fn alloc_range_dpa(&mut self, range: &mut [PhyAddr4K]) -> Result<(), FrameOutOfMemory> {
        self.alloc_range_impl(range, |a| a.into())
    }

    fn dealloc_dpa(&mut self, data: PhyAddr4K) {
        self.dealloc(data.into())
    }

    fn dealloc_range_dpa(&mut self, range: &[PhyAddr4K]) {
        range.iter().for_each(|&a| self.recycled.push(a.into()));
    }
}

type FrameAllocatorImpl = StackGlobalFrameAllocator;

static FRAME_ALLOCATOR: SpinLock<FrameAllocatorImpl> = SpinLock::new(FrameAllocatorImpl::new());

pub fn init_frame_allocator() {
    extern "C" {
        fn end();
    }
    println!("[FTL OS]init_frame_allocator");
    FRAME_ALLOCATOR.lock(place!()).init(
        PhyAddrRef::from(end as usize - KERNEL_OFFSET_FROM_DIRECT_MAP).ceil(),
        PhyAddrRef::from(INIT_MEMORY_END - KERNEL_OFFSET_FROM_DIRECT_MAP).floor(),
    );
}
pub fn size() -> usize {
    FRAME_ALLOCATOR.lock(place!()).size()
}

pub fn alloc() -> Result<FrameTracker, FrameOutOfMemory> {
    FRAME_ALLOCATOR
        .lock(place!())
        .alloc()
        .map(|a| unsafe { FrameTracker::new(a) })
}

pub fn alloc_range_to(range: &mut [PhyAddrRef4K]) -> Result<(), FrameOutOfMemory> {
    FRAME_ALLOCATOR.lock(place!()).alloc_range(range)
}

pub unsafe fn dealloc(par: PhyAddrRef4K) {
    FRAME_ALLOCATOR.lock(place!()).dealloc(par);
}

pub unsafe fn dealloc_range_from(range: &mut [PhyAddrRef4K]) {
    FRAME_ALLOCATOR.lock(place!()).dealloc_range(range);
}

pub fn alloc_dpa() -> Result<FrameTrackerDpa, FrameOutOfMemory> {
    FRAME_ALLOCATOR
        .lock(place!())
        .alloc_dpa()
        .map(|a| unsafe { FrameTrackerDpa::new(a) })
}

pub fn alloc_range_dpa_to(range: &mut [PhyAddr4K]) -> Result<(), FrameOutOfMemory> {
    FRAME_ALLOCATOR.lock(place!()).alloc_range_dpa(range)
}

pub unsafe fn dealloc_dpa(pa: PhyAddr4K) {
    FRAME_ALLOCATOR.lock(place!()).dealloc_dpa(pa);
}

pub unsafe fn dealloc_range_dpa_from(range: &mut [PhyAddr4K]) {
    FRAME_ALLOCATOR.lock(place!()).dealloc_range_dpa(range);
}
