use super::error::FrameOOM;
#[macro_use]
pub mod from_usize_allocator;

/// release T when drop
pub trait Own<T> {}

impl<T> Own<T> for T {}
/// the type of alloc and dealloc is the same.
pub trait RawAllocator<T> {
    fn alloc(&mut self) -> Result<T, FrameOOM>;
    unsafe fn dealloc(&mut self, value: T);
}
pub trait TrackerAllocator<T, H: Own<T>>: Send + Sync + 'static {
    fn alloc(&mut self) -> Result<H, FrameOOM>;
    unsafe fn dealloc(&mut self, value: T);
    fn alloc_directory(&mut self) -> Result<H, FrameOOM>;
    unsafe fn dealloc_directory(&mut self, value: T);
}
