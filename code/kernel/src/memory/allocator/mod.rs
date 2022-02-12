pub mod frame;
mod heap;

pub use heap::global_heap_alloc;
pub use heap::global_heap_dealloc;

pub fn init() {
    frame::init_frame_allocator();
    heap::init_heap();
}
