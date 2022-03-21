use core::{alloc::Layout, ptr::NonNull};

use crate::{config::PAGE_SIZE, tools::container::intrusive_linked_list::IntrusiveLinkedList};

// 这个堆不需要加锁，但需要增加访问标志，即使用时进入中断则需要从全局分配器访问.
//
// 缓存 2KB 及以下的空间, 2KB = 2^11
pub struct LocalHeap {
    free_list: [IntrusiveLinkedList; 12],
    in_used: bool,
}

const MAX_CLASS: usize = 11;
const CACHE_SIZE: usize = PAGE_SIZE;

// automatic modify LocalHeap used flag
macro_rules! try_local_or {
    ($self: expr, $global_run: expr) => {
        let _flag = match $self.try_using() {
            Err(_) => return { $global_run },
            Ok(flag) => flag,
        };
    };
}

impl LocalHeap {
    pub const fn new() -> Self {
        const LIST: IntrusiveLinkedList = IntrusiveLinkedList::new();
        Self {
            free_list: [LIST; 12],
            in_used: false,
        }
    }
    fn max_cache_size(size_log2: usize) -> usize {
        let xsize = CACHE_SIZE / (1 << size_log2);
        xsize.max(16).min(256)
    }
    fn try_using(&mut self) -> Result<impl Drop, ()> {
        struct AutoUsed {
            ptr: *mut LocalHeap,
        }
        impl Drop for AutoUsed {
            fn drop(&mut self) {
                unsafe {
                    assert!((*self.ptr).in_used);
                    (*self.ptr).in_used = false;
                }
            }
        }
        if self.in_used {
            return Err(());
        }
        self.in_used = true;
        Ok(AutoUsed {
            ptr: self as *const _ as *mut _,
        })
    }

    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let (_size, class) = super::layout_info(layout);

        if class > MAX_CLASS {
            return super::global_heap_alloc(layout);
        }

        try_local_or!(self, super::global_heap_alloc(layout));

        let list = &mut self.free_list[class];

        if let Some(ptr) = list.pop() {
            return Ok(ptr.cast());
        }
        let load_size = Self::max_cache_size(class) / 2;
        let mut new_list = super::global_heap_alloc_list(layout, load_size)?;

        list.append(&mut new_list);
        
        Ok(list.pop().unwrap().cast())
    }

    pub fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let (_size, class) = super::layout_info(layout);

        if class > MAX_CLASS {
            return super::global_heap_dealloc(ptr, layout);
        }

        try_local_or!(self, super::global_heap_dealloc(ptr, layout));

        let list = &mut self.free_list[class];
        unsafe {
            list.push(ptr.cast());
            let store_size = Self::max_cache_size(class);
            if list.len() >= store_size {
                list.size_check().unwrap();
                let store_list = list.take(store_size / 2);
                list.size_check().unwrap();
                store_list.size_check().unwrap();
                super::global_heap_dealloc_list(store_list, layout);
            }
        }
    }
}
