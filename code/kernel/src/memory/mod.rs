pub mod address;
pub mod allocator;
pub mod asid;
mod page_table;
pub mod map_segment;
pub mod user_ptr;
mod user_space;

pub use page_table::{set_satp_by_global, PageTable, PageTableClosed};
pub use user_space::{stack::StackID, UserSpace};

pub fn init() {
    allocator::init();
    page_table::init_kernel_page_table();
    asid::asid_test();
}
