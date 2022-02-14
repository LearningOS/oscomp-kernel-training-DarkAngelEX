pub mod frame;
mod heap;

use core::alloc::Layout;

pub use heap::global_heap_alloc;
pub use heap::global_heap_dealloc;

use crate::config::PAGE_SIZE;
use crate::tools::error::HeapOutOfMemory;
use crate::tools::FailRun;

pub fn heap_space_enough() -> Result<(), HeapOutOfMemory> {
    let layout = Layout::from_size_align(PAGE_SIZE * 4, PAGE_SIZE * 4).unwrap();
    let p1 = global_heap_alloc(layout)?;
    let f1 = FailRun::new(|| {
        println!("heap no enough! continuous space < 16K");
        global_heap_dealloc(p1, layout)
    });
    let p2 = global_heap_alloc(layout)?;
    let f2 = FailRun::new(|| {
        println!("heap no enough! continuous space < 32K");
        global_heap_dealloc(p2, layout)
    });
    global_heap_dealloc(p1, layout);
    global_heap_dealloc(p2, layout);
    f1.consume();
    f2.consume();
    Ok(())
}

pub fn init() {
    frame::init_frame_allocator();
    heap::init_heap();
}
