mod address;
mod frame_allocator;
mod heap_allocator;
mod page_table;

pub use page_table::new_kernel_page_table;

use crate::riscv::csr;

pub fn init() {
    heap_allocator::init_heap();
    frame_allocator::init_frame_allocator();
    println!("try load page table.");
    let page_table = crate::mm::new_kernel_page_table().expect("new kernel page table error.");
    unsafe {
        csr::set_satp(page_table.satp());
        csr::sfence_vma_all_global();
    }
}
