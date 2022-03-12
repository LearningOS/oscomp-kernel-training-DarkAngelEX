use core::{
    alloc::Layout,
    cmp::{max, min},
    mem::size_of,
    ptr::NonNull,
};

use crate::tools::container::intrusive_linked_list::IntrusiveLinkedList;

use super::powerful_list::PowerfulList;

pub struct GlobalHeap {
    free_list: [PowerfulList; 32],
    used: usize,
    allocated: usize,
    total: usize,
}

impl GlobalHeap {
    pub const fn new() -> Self {
        const LIST: PowerfulList = PowerfulList::new();
        Self {
            free_list: [LIST; 32],
            used: 0,
            allocated: 0,
            total: 0,
        }
    }
    pub fn init(&self, start: usize, size: usize) {}
    /// Add a range of memory [start, end) to the heap
    pub unsafe fn add_to_heap(&mut self, mut start: usize, mut end: usize) {
        // avoid unaligned access on some platforms
        start = (start + size_of::<usize>() - 1) & (!size_of::<usize>() + 1);
        end = end & (!size_of::<usize>() + 1);
        assert!(start <= end);

        let mut total = 0;
        let mut current_start = start;

        while current_start + size_of::<usize>() <= end {
            let lowbit = current_start & (!current_start + 1);
            let size = min(lowbit, prev_power_of_two(end - current_start));
            total += size;
            self.free_list[size.trailing_zeros() as usize]
                .push(NonNull::new_unchecked(current_start as *mut _));
            current_start += size;
        }
        self.total += total;
    }
    /// Alloc a range of memory from the heap satifying `layout` requirements
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let size = max(
            layout.size().next_power_of_two(),
            max(layout.align(), size_of::<usize>()),
        );
        let class = size.trailing_zeros() as usize;
        for i in class..self.free_list.len() {
            // Find the first non-empty size class
            if !self.free_list[i].is_empty() {
                // Split buffers
                for j in (class + 1..i + 1).rev() {
                    if let Some(block) = self.free_list[j].pop() {
                        unsafe {
                            self.free_list[j - 1].push(NonNull::new_unchecked(
                                (block.as_ptr() as usize + (1 << (j - 1))) as *mut usize,
                            ));
                            self.free_list[j - 1].push(block);
                        }
                    } else {
                        return Err(());
                    }
                }
                let result = self.free_list[class].pop().unwrap();
                self.used += layout.size();
                self.allocated += size;
                return Ok(result.cast());
            }
        }
        Err(())
    }

    pub fn dealloc(&mut self, ptr: NonNull<u8>, layout: Layout) {
        let size = max(
            layout.size().next_power_of_two(),
            max(layout.align(), size_of::<usize>()),
        );
        let class = size.trailing_zeros() as usize;

        unsafe {
            // Merge free buddy lists
            let mut current_class = class;
            let mut temp_list = IntrusiveLinkedList::new();
            temp_list.push(ptr.cast());
            while current_class < self.free_list.len() {
                let stop = current_class + 1 != self.free_list.len();
                temp_list =
                    match self.free_list[current_class].maybe_collection(temp_list, current_class, stop)
                    {
                        Some(list) => list,
                        None => break,
                    };
                current_class += 1;
            }
        }

        self.used -= layout.size();
        self.allocated -= size;
    }
}

fn prev_power_of_two(num: usize) -> usize {
    1 << (8 * (core::mem::size_of::<usize>()) - num.leading_zeros() as usize - 1)
}
