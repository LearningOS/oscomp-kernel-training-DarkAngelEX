use core::alloc::Layout;

use crate::{
    config::PAGE_SIZE,
    tools::{error::HeapOOM, FailRun},
};

pub mod frame;
mod heap;

pub use heap::local_heap::LocalHeap;

pub fn heap_space_enough() -> Result<(), HeapOOM> {
    let layout = Layout::from_size_align(PAGE_SIZE * 4, PAGE_SIZE * 4).unwrap();
    let p1 = heap::global_heap_alloc(layout)?;
    let f1 = FailRun::new(|| {
        println!("heap no enough! continuous space < 16K");
        heap::global_heap_dealloc(p1, layout)
    });
    let p2 = heap::global_heap_alloc(layout)?;
    let f2 = FailRun::new(|| {
        println!("heap no enough! continuous space < 32K");
        heap::global_heap_dealloc(p2, layout)
    });
    heap::global_heap_dealloc(p1, layout);
    heap::global_heap_dealloc(p2, layout);
    f1.consume();
    f2.consume();
    Ok(())
}

pub fn init() {
    frame::global::init_frame_allocator();
    heap::init_heap();
}
