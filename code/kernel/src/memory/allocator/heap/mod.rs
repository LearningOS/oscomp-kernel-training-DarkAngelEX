use core::{
    alloc::{GlobalAlloc, Layout},
    ptr::NonNull,
};

use self::gc_heap::DelayGCHeap;
use crate::{
    config::{KERNEL_HEAP_SIZE, KERNEL_OFFSET_FROM_DIRECT_MAP},
    local,
    sync::mutex::SpinNoIrqLock,
    tools::container::intrusive_linked_list::IntrusiveLinkedList,
    xdebug::{CLOSE_HEAP_DEALLOC, CLOSE_LOCAL_HEAP, HEAP_DEALLOC_OVERWRITE},
};

mod delay_gc_list;
mod gc_heap;
pub mod local_heap;

struct GlobalHeap {
    heap: SpinNoIrqLock<DelayGCHeap>,
}

unsafe impl GlobalAlloc for GlobalHeap {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ret = if !CLOSE_LOCAL_HEAP {
            local::hart_local()
                .local_heap
                .alloc(layout)
                .unwrap()
                .as_ptr()
        } else {
            self.alloc(layout).unwrap().as_ptr()
        };
        ret
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if HEAP_DEALLOC_OVERWRITE {
            let arr = core::slice::from_raw_parts_mut(ptr, layout.size());
            arr.fill(0xf0);
        }
        if CLOSE_HEAP_DEALLOC {
            return;
        }
        if !CLOSE_LOCAL_HEAP {
            local::hart_local()
                .local_heap
                .dealloc(NonNull::new(ptr).unwrap(), layout);
        } else {
            self.dealloc(NonNull::new(ptr).unwrap(), layout)
        }
    }
}

impl GlobalHeap {
    pub fn alloc(&self, layout: Layout) -> Result<NonNull<u8>, ()> {
        self.heap.lock(place!()).alloc(layout)
    }
    pub fn dealloc(&self, ptr: NonNull<u8>, layout: Layout) {
        self.heap.lock(place!()).dealloc(ptr, layout)
    }
    pub fn alloc_list(&self, layout: Layout, n: usize) -> Result<IntrusiveLinkedList, ()> {
        self.heap.lock(place!()).alloc_list(layout, n)
    }
    pub fn dealloc_list(&self, list: IntrusiveLinkedList, layout: Layout) {
        self.heap.lock(place!()).dealloc_list(list, layout)
    }
}

impl GlobalHeap {
    pub const fn empty() -> Self {
        Self {
            heap: SpinNoIrqLock::new(DelayGCHeap::empty()),
        }
    }
}

#[global_allocator]
static HEAP_ALLOCATOR: GlobalHeap = GlobalHeap::empty();

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init_heap() {
    println!("[FTL OS]init_heap");
    unsafe {
        HEAP_ALLOCATOR.heap.lock(place!()).init(
            HEAP_SPACE.as_ptr() as usize - KERNEL_OFFSET_FROM_DIRECT_MAP,
            KERNEL_HEAP_SIZE,
        );
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}

pub fn global_heap_alloc(layout: Layout) -> Result<NonNull<u8>, ()> {
    HEAP_ALLOCATOR.alloc(layout)
}

pub fn global_heap_alloc_list(layout: Layout, n: usize) -> Result<IntrusiveLinkedList, ()> {
    HEAP_ALLOCATOR.alloc_list(layout, n)
}

pub fn global_heap_dealloc(ptr: NonNull<u8>, layout: Layout) {
    HEAP_ALLOCATOR.dealloc(ptr, layout)
}

pub fn global_heap_dealloc_list(list: IntrusiveLinkedList, layout: Layout) {
    HEAP_ALLOCATOR.dealloc_list(list, layout)
}

// return (size, align_log2)
fn layout_info(layout: Layout) -> (usize, usize) {
    let size = layout
        .size()
        .next_power_of_two()
        .max(layout.align())
        .max(core::mem::size_of::<usize>());
    (size, size.trailing_zeros() as usize)
}
