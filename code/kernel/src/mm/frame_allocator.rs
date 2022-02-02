use crate::config::PAGE_SIZE;
use alloc::vec::Vec;
///
/// this module will alloc frame(4KB)
use core::{default::default, fmt::Debug};

use crate::{
    config::{INIT_MEMORY_END, KERNEL_OFFSET_FROM_DIRECT_MAP},
    mm::address::PhyAddrRef,
    sync::mutex::SpinLock,
};

use super::address::{PhyAddrMasked, PhyAddrRefMasked, StepByOne};
use lazy_static::lazy_static;

#[derive(Debug)]
pub struct FrameTracker {
    data: PhyAddrRefMasked,
}

impl FrameTracker {
    pub unsafe fn new(data: PhyAddrRefMasked) -> Self {
        Self { data }
    }
    pub fn data(&self) -> PhyAddrRefMasked {
        self.data
    }
    pub fn consume(self) -> PhyAddrRefMasked {
        let data = self.data;
        core::mem::forget(self);
        data
    }
}

impl Drop for FrameTracker {
    fn drop(&mut self) {
        unsafe { frame_dealloc(self.data) };
    }
}

pub struct FrameTrackerDpa {
    data: PhyAddrMasked,
}

impl FrameTrackerDpa {
    pub unsafe fn new(data: PhyAddrMasked) -> Self {
        Self { data }
    }
    pub fn data(&self) -> PhyAddrMasked {
        self.data
    }
    pub fn consume(self) -> PhyAddrMasked {
        let data = self.data;
        core::mem::forget(self);
        data
    }
}

impl Drop for FrameTrackerDpa {
    fn drop(&mut self) {
        unsafe { frame_dealloc_dpa(self.data) };
    }
}

trait FrameAllocator {
    fn new() -> Self;
    fn alloc(&mut self) -> Result<PhyAddrRefMasked, ()>;
    fn dealloc(&mut self, data: PhyAddrRefMasked);
    fn alloc_range(&mut self, range: &mut [PhyAddrRefMasked]) -> Result<(), ()>;
    fn dealloc_range(&mut self, range: &[PhyAddrRefMasked]);
    fn alloc_dpa(&mut self) -> Result<PhyAddrMasked, ()>;
    fn dealloc_dpa(&mut self, data: PhyAddrMasked);
    fn alloc_range_dpa(&mut self, range: &mut [PhyAddrMasked]) -> Result<(), ()>;
    fn dealloc_range_dpa(&mut self, range: &[PhyAddrMasked]);
}

pub struct StackFrameAllocator {
    begin: PhyAddrRefMasked, // used in recycle check.
    current: PhyAddrRefMasked,
    end: PhyAddrRefMasked,
    recycled: Vec<PhyAddrRefMasked>,
}

impl StackFrameAllocator {
    pub fn init(&mut self, begin: PhyAddrRefMasked, end: PhyAddrRefMasked) {
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
        f_tran: impl Fn(PhyAddrRefMasked) -> T,
    ) -> Result<(), ()> {
        let n = range.len();
        let n0 = (usize::from(self.end) - usize::from(self.current)) / PAGE_SIZE;
        if n0 + self.recycled.len() < n {
            return Err(());
        }
        let nt = n.min(n0);
        for (i, pa) in range.iter_mut().take(nt).enumerate() {
            *pa = f_tran(self.current.add_n_pg(i));
        }
        self.current = self.current.add_n_pg(nt);
        if n == nt {
            return Ok(());
        }
        for i in 0..n - nt {
            range[nt + i] = f_tran(self.recycled.pop().unwrap());
        }
        Ok(())
    }
}
impl FrameAllocator for StackFrameAllocator {
    fn new() -> Self {
        Self {
            begin: default(),
            current: default(),
            end: default(),
            recycled: Vec::new(),
        }
    }
    fn alloc(&mut self) -> Result<PhyAddrRefMasked, ()> {
        if let Some(pam) = self.recycled.pop() {
            Ok(pam)
        } else if self.current == self.end {
            Err(())
        } else {
            let ret = self.current;
            self.current.step();
            Ok(ret)
        }
    }
    fn alloc_range(&mut self, range: &mut [PhyAddrRefMasked]) -> Result<(), ()> {
        self.alloc_range_impl(range, |a| a)
    }
    fn dealloc(&mut self, data: PhyAddrRefMasked) {
        // O(N) validity check
        // debug_check!(
        //     data < self.current && self.recycled.iter().all(|&v| v != data),
        //     "Frame pam = {:?} has not been allocated!",
        //     data
        // );

        // recycle
        self.recycled.push(data);
    }

    fn dealloc_range(&mut self, range: &[PhyAddrRefMasked]) {
        range.iter().for_each(|&a| self.recycled.push(a));
    }

    fn alloc_dpa(&mut self) -> Result<PhyAddrMasked, ()> {
        self.alloc().map(|a| a.into())
    }

    fn alloc_range_dpa(&mut self, range: &mut [PhyAddrMasked]) -> Result<(), ()> {
        self.alloc_range_impl(range, |a| a.into())
    }

    fn dealloc_dpa(&mut self, data: PhyAddrMasked) {
        self.dealloc(data.into())
    }

    fn dealloc_range_dpa(&mut self, range: &[PhyAddrMasked]) {
        range.iter().for_each(|&a| self.recycled.push(a.into()));
    }
}

type FrameAllocatorImpl = StackFrameAllocator;

lazy_static! {
    pub static ref FRAME_ALLOCATOR: SpinLock<FrameAllocatorImpl> =
        SpinLock::new(FrameAllocatorImpl::new());
}

pub fn init_frame_allocator() {
    extern "C" {
        fn end();
    }
    FRAME_ALLOCATOR.lock().init(
        PhyAddrRef::from(end as usize - KERNEL_OFFSET_FROM_DIRECT_MAP).ceil(),
        PhyAddrRef::from(INIT_MEMORY_END - KERNEL_OFFSET_FROM_DIRECT_MAP).floor(),
    );
}

pub fn frame_alloc() -> Result<FrameTracker, ()> {
    FRAME_ALLOCATOR
        .lock()
        .alloc()
        .map(|a| unsafe { FrameTracker::new(a) })
}

pub fn frame_range_alloc(range: &mut [PhyAddrRefMasked]) -> Result<(), ()> {
    FRAME_ALLOCATOR.lock().alloc_range(range)
}

pub unsafe fn frame_dealloc(parm: PhyAddrRefMasked) {
    FRAME_ALLOCATOR.lock().dealloc(parm);
}

pub unsafe fn frame_range_dealloc(range: &mut [PhyAddrRefMasked]) {
    FRAME_ALLOCATOR.lock().dealloc_range(range);
}

pub fn frame_alloc_dpa() -> Result<FrameTrackerDpa, ()> {
    FRAME_ALLOCATOR
        .lock()
        .alloc_dpa()
        .map(|a| unsafe { FrameTrackerDpa::new(a) })
}

pub fn frame_range_alloc_dpa(range: &mut [PhyAddrMasked]) -> Result<(), ()> {
    FRAME_ALLOCATOR.lock().alloc_range_dpa(range)
}

pub unsafe fn frame_dealloc_dpa(pam: PhyAddrMasked) {
    FRAME_ALLOCATOR.lock().dealloc_dpa(pam);
}

pub unsafe fn frame_range_dealloc_dpa(range: &mut [PhyAddrMasked]) {
    FRAME_ALLOCATOR.lock().dealloc_range_dpa(range);
}

#[allow(unused)]
pub fn frame_allocator_test() {
    let mut v: Vec<FrameTracker> = Vec::new();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    v.clear();
    for i in 0..5 {
        let frame = frame_alloc().unwrap();
        println!("{:?}", frame);
        v.push(frame);
    }
    drop(v);
    println!("frame_allocator_test passed!");
}
