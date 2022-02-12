//! global heap
use core::{alloc::Layout, ptr::NonNull};

use crate::{config::KERNEL_HEAP_SIZE, tools::error::HeapOutOfMemory};
use buddy_system_allocator::LockedHeap;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init_heap() {
    println!("[FTL OS]init_heap");
    unsafe {
        HEAP_ALLOCATOR
            .lock()
            .init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

pub fn global_heap_alloc(layout: Layout) -> Result<NonNull<u8>, HeapOutOfMemory> {
    HEAP_ALLOCATOR
        .lock()
        .alloc(layout)
        .map_err(|_| HeapOutOfMemory)
}

pub fn global_heap_dealloc(ptr: NonNull<u8>, layout: Layout) {
    HEAP_ALLOCATOR.lock().dealloc(ptr, layout)
}
