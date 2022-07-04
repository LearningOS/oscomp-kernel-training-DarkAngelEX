use core::alloc::Layout;

use crate::xdebug::{HEAP_PROTECT, HEAP_RELEASE_CHECK};

const MIN_SIZE: usize = core::mem::size_of::<usize>();

pub fn detect_layout(layout: Layout) -> Layout {
    if HEAP_RELEASE_CHECK || HEAP_PROTECT {
        let align = layout.align().max(MIN_SIZE);
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
            ptr.add(layout.align().max(MIN_SIZE))
        }
    } else {
        ptr
    }
}
pub fn dealloc_run(ptr: *mut u8, layout: Layout) -> (*mut u8, Layout) {
    if HEAP_RELEASE_CHECK || HEAP_PROTECT {
        unsafe {
            let ptr = ptr.sub(layout.align().max(MIN_SIZE));
            if HEAP_RELEASE_CHECK {
                assert_eq!(*ptr, 100);
            }
            (ptr, detect_layout(layout))
        }
    } else {
        (ptr, layout)
    }
}
