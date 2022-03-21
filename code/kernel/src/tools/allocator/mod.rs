use super::error::FrameOutOfMemory;
#[macro_use]
pub mod from_usize_allocator;

/// release T when drop
pub trait Own<T> {}

impl<T> Own<T> for T {}
/// the type of alloc and dealloc is the same.
pub trait RawAllocator<T> {
    fn alloc(&mut self) -> Result<T, FrameOutOfMemory>;
    unsafe fn dealloc(&mut self, value: T);
}
pub trait TrackerAllocator<T, H: Own<T>> {
    fn alloc(&mut self) -> Result<H, FrameOutOfMemory>;
    unsafe fn dealloc(&mut self, value: T);
}
