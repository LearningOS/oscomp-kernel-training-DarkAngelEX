use crate::tools::error::FrameOutOfMemory;

use super::{address::PhyAddrRef4K, allocator};

use allocator::frame;

// manager
struct FrameTracker<const N: usize> {
    ptr: PhyAddrRef4K,
}

impl<const N: usize> FrameTracker<N> {
    pub fn new() -> Result<Self, FrameOutOfMemory> {
        if N != 1 {
            todo!("frame_track N = {N} is implement")
        }
        let ptr = frame::alloc()?.consume();
        Ok(Self { ptr })
    }
    pub fn ptr(&self) -> PhyAddrRef4K {
        self.ptr
    }
}

impl<const N: usize> Drop for FrameTracker<N> {
    fn drop(&mut self) {
        if N != 1 {
            todo!("frame_track N = {N} is implement")
        }
        unsafe { frame::dealloc(self.ptr) }
    }
}
