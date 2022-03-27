pub mod address;
pub mod allocator;
pub mod asid;
pub mod map_segment;
mod page_table;
pub mod user_ptr;
mod user_space;
pub mod auxv;

pub use page_table::{set_satp_by_global, PageTable, PageTableClosed};
pub use user_space::{stack::StackID, AccessType, UserSpace};

pub fn init() {
    allocator::init();
    page_table::init_kernel_page_table();
    asid::asid_test();
}
