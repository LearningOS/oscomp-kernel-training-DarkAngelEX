mod address;
mod frame_allocator;
mod heap_allocator;
mod page_table;

pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    page_table::init_kernel_page_table();
}
