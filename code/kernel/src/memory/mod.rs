pub mod address;
pub mod allocator;
pub mod asid;
pub mod auxv;
pub mod map_segment;
mod page_table;
pub mod rcu;
pub mod user_ptr;
mod user_space;

pub use map_segment::zero_copy::own_try_handle;
pub use page_table::{set_satp_by_global, PTEFlags, PageTable, PageTableClosed};
pub use user_space::{AccessType, UserSpace};
pub fn init() {
    allocator::init();
    page_table::init_kernel_page_table();
    asid::asid_test();
    rcu::init();
}
