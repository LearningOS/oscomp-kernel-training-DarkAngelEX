pub mod frame;
mod heap;

pub fn init() {
    frame::init_frame_allocator();
    heap::init_heap();
}