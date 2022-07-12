//! frame allocator which can be used in stack.

pub mod global;
pub mod iter;
pub mod detector;

use crate::{
    memory::address::PhyAddrRef4K,
    tools::{allocator::TrackerAllocator, error::FrameOOM},
};

use self::global::FrameTracker;

pub trait FrameAllocator = TrackerAllocator<PhyAddrRef4K, FrameTracker>;

#[inline]
pub fn defualt_allocator() -> impl FrameAllocator {
    GlobalRefFrameAllocator::new()
}

struct GlobalRefFrameAllocator;

impl TrackerAllocator<PhyAddrRef4K, FrameTracker> for GlobalRefFrameAllocator {
    #[inline]
    fn alloc(&mut self) -> Result<FrameTracker, FrameOOM> {
        global::alloc()
    }
    #[inline]
    unsafe fn dealloc(&mut self, value: PhyAddrRef4K) {
        global::dealloc(value)
    }
}

impl GlobalRefFrameAllocator {
    #[inline]
    pub fn new() -> Self {
        Self
    }
}
