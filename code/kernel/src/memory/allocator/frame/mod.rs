//! frame allocator which can be used in stack.

mod global;
pub mod iter;

pub use global::*;

use crate::{memory::address::PhyAddrRef4K, tools::{allocator::TrackerAllocator, error::FrameOutOfMemory}};

pub trait FrameAllocator = TrackerAllocator<PhyAddrRef4K, FrameTracker>;

pub fn defualt_allocator() -> impl FrameAllocator {
    GlobalRefFrameAllocator
}

struct GlobalRefFrameAllocator;

impl TrackerAllocator<PhyAddrRef4K, FrameTracker> for GlobalRefFrameAllocator {
    fn alloc(&mut self) -> Result<FrameTracker, FrameOutOfMemory> {
        alloc()
    }

    unsafe fn dealloc(&mut self, value: PhyAddrRef4K) {
        dealloc(value)
    }
}

impl GlobalRefFrameAllocator {
    pub fn new() -> Self {
        Self
    }
}
