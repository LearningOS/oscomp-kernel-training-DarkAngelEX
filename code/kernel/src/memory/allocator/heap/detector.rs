use core::alloc::Layout;

use crate::xdebug::{HEAP_PROTECT, HEAP_RELEASE_CHECK};
fn xsize() -> usize {
    core::mem::size_of::<usize>()
}
pub fn detect_layout(layout: Layout) -> Layout {
    if HEAP_RELEASE_CHECK || HEAP_PROTECT {
        let align = layout.align().max(xsize());
        let size = layout.size() + align;
        unsafe { Layout::from_size_align_unchecked(size, align) }
    } else {
        layout
    }
}

pub fn alloc_run(ptr: *mut u8, layout: Layout) -> *mut u8 {
    if HEAP_RELEASE_CHECK || HEAP_PROTECT {
        unsafe {
            if HEAP_RELEASE_CHECK {
                *ptr = 100;
            }
            ptr.add(layout.align().max(xsize()))
        }
    } else {
        ptr
    }
}
pub fn dealloc_run(ptr: *mut u8, layout: Layout) -> (*mut u8, Layout) {
    if HEAP_RELEASE_CHECK || HEAP_PROTECT {
        unsafe {
            let ptr = ptr.sub(layout.align().max(xsize()));
            if HEAP_RELEASE_CHECK {
                assert_eq!(*ptr, 100);
            }
            (ptr, detect_layout(layout))
        }
    } else {
        (ptr, layout)
    }
}
