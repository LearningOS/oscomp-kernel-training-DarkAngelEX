pub mod address;
pub mod allocator;
pub mod asid;
pub mod manager;
mod page_table;
pub mod user_ptr;
mod user_space;

pub use page_table::set_satp_by_global;
use page_table::PageTable;
pub use page_table::PageTableClosed;
pub use user_space::{SpaceGuard, SpaceMark, StackID, USpaceCreateError, UserSpace};

pub fn init() {
    allocator::init();
    page_table::init_kernel_page_table();
    asid::asid_test();
}
