use super::{address::PhyAddrRefMasked, frame_allocator};

// manager
struct FrameTracker<const N: usize> {
    ptr: PhyAddrRefMasked,
}

impl<const N: usize> FrameTracker<N> {
    pub fn new() -> Result<Self, ()> {
        if N != 1 {
            todo!("frame_track N = {N} is implement")
        }
        let ptr = frame_allocator::frame_alloc()?.consume();
        Ok(Self { ptr })
    }
    pub fn ptr(&self) -> PhyAddrRefMasked {
        self.ptr
    }
}

impl<const N: usize> Drop for FrameTracker<N> {
    fn drop(&mut self) {
        if N != 1 {
            todo!("frame_track N = {N} is implement")
        }
        unsafe { frame_allocator::frame_dealloc(self.ptr) }
    }
}
