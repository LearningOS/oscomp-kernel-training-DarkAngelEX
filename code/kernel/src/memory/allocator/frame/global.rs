//! Implementation of global allocator
//!
//! this module will alloc frame(4KB)
use crate::{
    config::{
        DIRECT_MAP_BEGIN, DIRECT_MAP_END, INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP, PAGE_SIZE,
    },
    memory::address::{PageCount, PhyAddr4K, PhyAddrRef, PhyAddrRef4K, StepByOne},
    sync::mutex::SpinNoIrqLock,
    tools::{
        allocator::Own,
        container::{never_clone_linked_list::NeverCloneLinkedList, Stack},
        error::FrameOutOfMemory,
    },
    xdebug::{
        trace::{self, OPEN_MEMORY_TRACE, TRACE_ADDR},
        CLOSE_FRAME_DEALLOC, FRAME_DEALLOC_OVERWRITE,
    },
};
use core::fmt::Debug;

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
    fn alloc_successive(&mut self, n: PageCount) -> Result<PhyAddrRef4K, FrameOutOfMemory>;
    fn alloc_iter<'a>(
        &mut self,
        range: impl Iterator<Item = &'a mut PhyAddrRef4K> + ExactSizeIterator,
    ) -> Result<(), FrameOutOfMemory>;
    fn dealloc_iter<'a>(&mut self, range: impl Iterator<Item = &'a PhyAddrRef4K>);
    fn alloc_dpa(&mut self) -> Result<PhyAddr4K, FrameOutOfMemory> {
        self.alloc().map(|p| p.into())
    }
    fn dealloc_dpa(&mut self, data: PhyAddr4K) {
        self.dealloc(data.into_ref())
    }
}

struct StackGlobalFrameAllocator {
    begin: PhyAddrRef4K, // used in recycle check.
    current: PhyAddrRef4K,
    end: PhyAddrRef4K,
    recycled: NeverCloneLinkedList<PhyAddrRef4K>,
}

impl StackGlobalFrameAllocator {
    const fn new() -> Self {
        Self {
            begin: unsafe { PhyAddrRef4K::from_usize(0) },
            current: unsafe { PhyAddrRef4K::from_usize(0) },
            end: unsafe { PhyAddrRef4K::from_usize(0) },
            recycled: NeverCloneLinkedList::new(),
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
}
impl GlobalFrameAllocator for StackGlobalFrameAllocator {
    fn size(&self) -> usize {
        self.recycled.len() + (self.end.into_usize() - self.current.into_usize()) / PAGE_SIZE
    }
    fn alloc(&mut self) -> Result<PhyAddrRef4K, FrameOutOfMemory> {
        fn pa_check(pa: PhyAddrRef4K) {
            assert!(pa.into_usize() > DIRECT_MAP_BEGIN && pa.into_usize() < DIRECT_MAP_END);
            if OPEN_MEMORY_TRACE && pa == PhyAddrRef::from(TRACE_ADDR).floor() {
                trace::call_when_alloc();
            }
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
            Ok(ret)
        }
    }

    fn dealloc(&mut self, data: PhyAddrRef4K) {
        if OPEN_MEMORY_TRACE && data == PhyAddrRef::from(TRACE_ADDR).floor() {
            trace::call_when_dealloc();
        }
        if FRAME_DEALLOC_OVERWRITE {
            let arr =
                unsafe { core::slice::from_raw_parts_mut(data.into_usize() as *mut u8, PAGE_SIZE) };
            arr.fill(0xf0);
        }
        if CLOSE_FRAME_DEALLOC {
            return;
        }
        // return;
        // skip null
        debug_check!(data.into_usize() % PAGE_SIZE == 0);
        assert!(data.into_usize() > DIRECT_MAP_BEGIN && data.into_usize() < DIRECT_MAP_END);
        self.recycled.push(data);
    }

    fn alloc_successive(&mut self, n: PageCount) -> Result<PhyAddrRef4K, FrameOutOfMemory> {
        match n.into_usize() {
            0 => panic!(),
            1 => return self.alloc(),
            _ => (),
        }
        let ret = self.current;
        let nxt = self.current.add_page(n);
        if nxt > self.end {
            return Err(FrameOutOfMemory);
        }
        self.current = nxt;
        Ok(ret)
    }

    fn alloc_iter<'a>(
        &mut self,
        mut range: impl Iterator<Item = &'a mut PhyAddrRef4K> + ExactSizeIterator,
    ) -> Result<(), FrameOutOfMemory> {
        let n = range.len();
        let n0 = (usize::from(self.end) - usize::from(self.current)) / PAGE_SIZE;
        if n0 + self.recycled.len() < n {
            return Err(FrameOutOfMemory);
        }
        while let Some(pa) = self.recycled.pop() {
            if let Some(target) = range.next() {
                *target = pa;
            } else {
                return Ok(());
            }
        }
        while let Some(target) = range.next() {
            *target = self.current;
            self.current.add_page_assign(PageCount::from_usize(1));
        }
        assert!(self.current <= self.end);
        Ok(())
    }

    fn dealloc_iter<'a>(&mut self, range: impl Iterator<Item = &'a PhyAddrRef4K>) {
        range.for_each(|&a| self.recycled.push(a));
    }
}

type FrameAllocatorImpl = StackGlobalFrameAllocator;

static FRAME_ALLOCATOR: SpinNoIrqLock<FrameAllocatorImpl> =
    SpinNoIrqLock::new(FrameAllocatorImpl::new());

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

pub fn alloc_successive(n: PageCount) -> Result<PhyAddrRef4K, FrameOutOfMemory> {
    FRAME_ALLOCATOR.lock(place!()).alloc_successive(n)
}

pub fn alloc_iter<'a>(
    range: impl Iterator<Item = &'a mut PhyAddrRef4K> + ExactSizeIterator,
) -> Result<(), FrameOutOfMemory> {
    FRAME_ALLOCATOR.lock(place!()).alloc_iter(range)
}

pub fn alloc_n<const N: usize>() -> Result<[FrameTracker; N], FrameOutOfMemory> {
    let mut t = [unsafe { PhyAddrRef4K::from_usize(0) }; N];
    alloc_iter(t.iter_mut())?;
    Ok(t.map(|a| unsafe { FrameTracker::new(a) }))
}

pub unsafe fn dealloc(par: PhyAddrRef4K) {
    FRAME_ALLOCATOR.lock(place!()).dealloc(par);
}

pub fn alloc_dpa() -> Result<FrameTrackerDpa, FrameOutOfMemory> {
    FRAME_ALLOCATOR
        .lock(place!())
        .alloc_dpa()
        .map(|a| unsafe { FrameTrackerDpa::new(a) })
}

pub unsafe fn dealloc_dpa(pa: PhyAddr4K) {
    FRAME_ALLOCATOR.lock(place!()).dealloc_dpa(pa);
}
