//! frame allocator which can be used in stack.

pub mod global;
pub mod iter;

use crate::{
    memory::address::PhyAddrRef4K,
    tools::{allocator::TrackerAllocator, error::FrameOutOfMemory},
};

use self::global::FrameTracker;

pub trait FrameAllocator = TrackerAllocator<PhyAddrRef4K, FrameTracker>;

pub fn defualt_allocator() -> impl FrameAllocator {
    GlobalRefFrameAllocator::new()
}

struct GlobalRefFrameAllocator;

impl TrackerAllocator<PhyAddrRef4K, FrameTracker> for GlobalRefFrameAllocator {
    fn alloc(&mut self) -> Result<FrameTracker, FrameOutOfMemory> {
        global::alloc()
    }

    unsafe fn dealloc(&mut self, value: PhyAddrRef4K) {
        global::dealloc(value)
    }
}

impl GlobalRefFrameAllocator {
    pub fn new() -> Self {
        Self
    }
}
