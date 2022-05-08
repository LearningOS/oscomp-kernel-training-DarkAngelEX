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
        error::FrameOOM,
    },
    xdebug::{
        trace::{self, OPEN_MEMORY_TRACE, TRACE_ADDR},
        CLOSE_FRAME_DEALLOC, FRAME_DEALLOC_OVERWRITE,
    },
};
use core::fmt::Debug;

use super::detector::FrameDetector;

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

trait GlobalFrameAllocator {
    /// return count of frame, neither space size.
    fn size(&self) -> usize;
    fn alloc(&mut self) -> Result<PhyAddrRef4K, FrameOOM>;
    fn dealloc(&mut self, data: PhyAddrRef4K);
    fn alloc_successive(&mut self, n: PageCount) -> Result<PhyAddrRef4K, FrameOOM>;
    fn alloc_iter<'a>(
        &mut self,
        range: impl Iterator<Item = &'a mut PhyAddrRef4K> + ExactSizeIterator,
    ) -> Result<(), FrameOOM>;
    fn dealloc_iter<'a>(&mut self, range: impl Iterator<Item = &'a PhyAddrRef4K>);
    fn alloc_dpa(&mut self) -> Result<PhyAddr4K, FrameOOM> {
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
    detector: FrameDetector,
}

impl StackGlobalFrameAllocator {
    const fn new() -> Self {
        Self {
            begin: unsafe { PhyAddrRef4K::from_usize(0) },
            current: unsafe { PhyAddrRef4K::from_usize(0) },
            end: unsafe { PhyAddrRef4K::from_usize(0) },
            recycled: NeverCloneLinkedList::new(),
            detector: FrameDetector::new(),
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
    fn alloc(&mut self) -> Result<PhyAddrRef4K, FrameOOM> {
        fn pa_check(pa: PhyAddrRef4K) {
            assert!(pa.into_usize() > DIRECT_MAP_BEGIN && pa.into_usize() < DIRECT_MAP_END);
            if OPEN_MEMORY_TRACE && pa == PhyAddrRef::<u8>::from(TRACE_ADDR).floor() {
                trace::call_when_alloc();
            }
        }
        let pa = if let Some(pa) = self.recycled.pop() {
            self.detector.alloc_run(pa);
            pa
        } else if self.current == self.end {
            return Err(FrameOOM);
        } else {
            let pa = self.current;
            self.current.step();
            pa
        };
        pa_check(pa);
        Ok(pa)
    }

    fn dealloc(&mut self, addr: PhyAddrRef4K) {
        if OPEN_MEMORY_TRACE && addr == PhyAddrRef::<u8>::from(TRACE_ADDR).floor() {
            trace::call_when_dealloc();
        }
        if FRAME_DEALLOC_OVERWRITE {
            let arr =
                unsafe { core::slice::from_raw_parts_mut(addr.into_usize() as *mut u8, PAGE_SIZE) };
            arr.fill(0xf0);
        }
        self.detector.dealloc_run(addr);
        if CLOSE_FRAME_DEALLOC {
            return;
        }
        // return;
        // skip null
        debug_assert!(addr.into_usize() % PAGE_SIZE == 0);
        assert!(addr.into_usize() > DIRECT_MAP_BEGIN && addr.into_usize() < DIRECT_MAP_END);
        self.recycled.push(addr);
    }

    fn alloc_successive(&mut self, n: PageCount) -> Result<PhyAddrRef4K, FrameOOM> {
        match n.into_usize() {
            0 => panic!(),
            1 => return self.alloc(),
            _ => (),
        }
        let ret = self.current;
        let nxt = self.current.add_page(n);
        if nxt > self.end {
            return Err(FrameOOM);
        }
        self.current = nxt;
        Ok(ret)
    }

    fn alloc_iter<'a>(
        &mut self,
        mut range: impl Iterator<Item = &'a mut PhyAddrRef4K> + ExactSizeIterator,
    ) -> Result<(), FrameOOM> {
        let n = range.len();
        let n0 = (usize::from(self.end) - usize::from(self.current)) / PAGE_SIZE;
        if n0 + self.recycled.len() < n {
            return Err(FrameOOM);
        }
        while let Some(pa) = self.recycled.pop() {
            if let Some(target) = range.next() {
                self.detector.alloc_run(pa);
                *target = pa;
            } else {
                return Ok(());
            }
        }
        for target in range {
            *target = self.current;
            self.current.add_page_assign(PageCount(1));
        }
        assert!(self.current <= self.end);
        Ok(())
    }

    fn dealloc_iter<'a>(&mut self, range: impl Iterator<Item = &'a PhyAddrRef4K>) {
        range.for_each(|&pa| {
            self.detector.dealloc_run(pa);
            self.recycled.push(pa)
        });
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
    FRAME_ALLOCATOR.lock().init(
        PhyAddrRef::<u8>::from(end as usize - KERNEL_OFFSET_FROM_DIRECT_MAP).ceil(),
        PhyAddrRef::<u8>::from(INIT_MEMORY_END - KERNEL_OFFSET_FROM_DIRECT_MAP).floor(),
    );
}
// pub fn size() -> usize {
//     FRAME_ALLOCATOR.lock().size()
// }

pub fn alloc() -> Result<FrameTracker, FrameOOM> {
    let v = FRAME_ALLOCATOR
        .lock()
        .alloc()
        .map(|a| unsafe { FrameTracker::new(a) })?;
    Ok(v)
}

pub fn alloc_successive(n: PageCount) -> Result<PhyAddrRef4K, FrameOOM> {
    FRAME_ALLOCATOR.lock().alloc_successive(n)
}

pub fn alloc_iter<'a>(
    range: impl Iterator<Item = &'a mut PhyAddrRef4K> + ExactSizeIterator,
) -> Result<(), FrameOOM> {
    FRAME_ALLOCATOR.lock().alloc_iter(range)
}

pub fn alloc_n<const N: usize>() -> Result<[FrameTracker; N], FrameOOM> {
    let mut t = [unsafe { PhyAddrRef4K::from_usize(0) }; N];
    alloc_iter(t.iter_mut())?;
    Ok(t.map(|a| unsafe { FrameTracker::new(a) }))
}

pub unsafe fn dealloc(par: PhyAddrRef4K) {
    FRAME_ALLOCATOR.lock().dealloc(par);
}
