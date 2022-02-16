pub mod address;
pub mod allocator;
pub mod asid;
pub mod error;
pub mod manager;
mod page_table;
mod user_space;
pub mod stack;

pub use page_table::set_satp_by_global;
use page_table::PageTable;
pub use user_space::{StackID, USpaceCreateError, UserSpace};

pub fn init() {
    allocator::init();
    page_table::init_kernel_page_table();
    asid::asid_test();
}
