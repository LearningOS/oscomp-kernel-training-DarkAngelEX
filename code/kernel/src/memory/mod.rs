mod address;
mod frame_allocator;
mod heap_allocator;
pub mod manager;
mod page_table;

pub use page_table::{set_satp_by_global, UserPageTable};

pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    page_table::init_kernel_page_table();
}
