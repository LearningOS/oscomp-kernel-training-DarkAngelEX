use core::{alloc::Layout, cmp, mem, ptr::NonNull};

use crate::{
    config::PAGE_SIZE,
    tools::{container::intrusive_linked_list::IntrusiveLinkedList, error::HeapOutOfMemory},
};

// 这个堆不需要加锁，但需要增加访问标志，即使用时进入中断则需要从全局分配器访问.
//
// 缓存 2KB 及以下的空间, 2KB = 2^11
pub struct LocalHeap {
    free_list: [IntrusiveLinkedList; 12],
    in_used: bool,
}

const MAX_CLASS: usize = 11;
const CACHE_SIZE: usize = PAGE_SIZE;
impl LocalHeap {
    pub const fn new() -> Self {
        const LIST: IntrusiveLinkedList = IntrusiveLinkedList::new();
        Self {
            free_list: [LIST; 12],
            in_used: false,
        }
    }
    const fn max_cache_size(size_log2: usize) -> usize {
        let mut xsize = CACHE_SIZE / (1 << size_log2);
        if xsize < 16 {
            xsize = 16;
        }
        if xsize > 256 {
            xsize = 256;
        }
        xsize
    }
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let size = cmp::max(
            layout.size().next_power_of_two(),
            cmp::max(layout.align(), mem::size_of::<usize>()),
        );
        let class = size.trailing_zeros() as usize;

        if class > MAX_CLASS {
            return super::global_heap_alloc(layout);
        }

        let list = &mut self.free_list[class];

        if let Some(ptr) = list.pop() {
            return Ok(ptr.cast());
        }

        let load_size = Self::max_cache_size(class);

        todo!();
    }
}
