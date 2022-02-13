//! global heap
use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};

use crate::{config::KERNEL_HEAP_SIZE, tools::error::HeapOutOfMemory};
use buddy_system_allocator::LockedHeap;

struct GlobalHeap {
    heap: LockedHeap,
}

unsafe impl GlobalAlloc for GlobalHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.alloc(layout).unwrap().as_ptr()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.dealloc(NonNull::new(ptr).unwrap(), layout)
    }
}

impl GlobalHeap {
    pub fn alloc(&self, layout: Layout) -> Result<NonNull<u8>, ()> {
        self.heap.lock().alloc(layout)
    }
    pub fn dealloc(&self, ptr: NonNull<u8>, layout: Layout) {
        self.heap.lock().dealloc(ptr, layout)
    }
}

impl GlobalHeap {
    pub const fn empty() -> Self {
        Self {
            heap: LockedHeap::empty(),
        }
    }
}

#[global_allocator]
static HEAP_ALLOCATOR: GlobalHeap = GlobalHeap::empty();

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init_heap() {
    println!("[FTL OS]init_heap");
    unsafe {
        HEAP_ALLOCATOR
            .heap
            .lock()
            .init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

pub fn global_heap_alloc(layout: Layout) -> Result<NonNull<u8>, HeapOutOfMemory> {
    HEAP_ALLOCATOR.alloc(layout).map_err(|_| HeapOutOfMemory)
}

pub fn global_heap_dealloc(ptr: NonNull<u8>, layout: Layout) {
    HEAP_ALLOCATOR.dealloc(ptr, layout)
}
