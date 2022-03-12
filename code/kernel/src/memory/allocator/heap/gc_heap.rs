use core::{alloc::Layout, cmp::min, mem::size_of, ptr::NonNull};

use crate::tools::container::intrusive_linked_list::IntrusiveLinkedList;

use super::delay_gc_list::DelayGCList;

pub struct DelayGCHeap {
    free_list: [DelayGCList; 32],
    used: usize,
    allocated: usize,
    total: usize,
}

impl DelayGCHeap {
    pub const fn empty() -> Self {
        const LIST: DelayGCList = DelayGCList::new();
        Self {
            free_list: [LIST; 32],
            used: 0,
            allocated: 0,
            total: 0,
        }
    }
    pub fn init(&mut self, start: usize, size: usize) {
        unsafe {
            self.add_to_heap(start, start + size, true);
        }
    }
    /// Add a range of memory [start, end) to the heap
    pub unsafe fn add_to_heap(&mut self, mut start: usize, mut end: usize, modify_total: bool) {
        // avoid unaligned access on some platforms
        const ALIGN_SIZE: usize = size_of::<usize>();

        start = (start + ALIGN_SIZE - 1) & (!ALIGN_SIZE + 1);
        end = end & (!ALIGN_SIZE + 1);
        assert!(start <= end, "{:#x} {:#x}", start, end);

        let mut total = 0;
        let mut current_start = start;

        while current_start + ALIGN_SIZE <= end {
            let lowbit = current_start & (!current_start + 1);
            let size = min(lowbit, prev_power_of_two(end - current_start));
            total += size;
            self.free_list[size.trailing_zeros() as usize]
                .push(NonNull::new_unchecked(current_start as *mut _));
            current_start += size;
        }
        if modify_total {
            self.total += total;
        }
    }

    /// Alloc a range of memory from the heap satifying `layout` requirements
    pub fn alloc(&mut self, layout: Layout) -> Result<NonNull<u8>, ()> {
        let (size, class) = super::layout_info(layout);
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
        unsafe {
            let mut temp_list = IntrusiveLinkedList::new();
            temp_list.push(ptr.cast());
            self.dealloc_list(temp_list, layout);
        }
    }
    pub fn alloc_list(&mut self, layout: Layout, n: usize) -> Result<IntrusiveLinkedList, ()> {
        let (size, class) = super::layout_info(layout);
        let mut cur_class = class;
        let mut take_all_class = 0;
        let mut rate = 1;
        let mut xn = n;
        let mut no_space = true;
        let mut ret_list = IntrusiveLinkedList::new();
        // take_all_class: 不大于此的链表会取走全部的内存
        // cur_class: 补全剩余零碎的空间
        while cur_class < self.free_list.len() {
            let list = &mut self.free_list[cur_class];
            let r_size = list.len() * rate;
            if xn <= r_size {
                no_space = false;
            }
            if xn >= r_size {
                take_all_class = cur_class;
                xn -= r_size;
            }
            if !no_space {
                break;
            }
            cur_class += 1;
            rate *= 2;
        }
        if no_space {
            return Err(());
        }
        if take_all_class != 0 {
            for xclass in class..=take_all_class {
                let list = &mut self.free_list[xclass];
                while let Some(a) = list.get_list().pop() {
                    let begin = a.as_ptr() as usize;
                    let end = begin + (1 << xclass);
                    ret_list.append(&mut IntrusiveLinkedList::from_range(begin, end, class));
                }
            }
        }
        let list = &mut self.free_list[cur_class];
        if xn != 0 {
            while xn >= 1 << cur_class - class {
                let a = list.get_list().pop().unwrap();
                let begin = a.as_ptr() as usize;
                let end = begin + (1 << cur_class);
                ret_list.append(&mut IntrusiveLinkedList::from_range(begin, end, class));
                xn -= 1 << cur_class - class;
            }
        }
        if xn != 0 {
            let last_page = self.free_list[cur_class].pop().unwrap();
            let page_begin = last_page.as_ptr() as usize;
            let page_mid = page_begin + (1 << class) * xn;
            let page_end = page_begin + (1 << cur_class);
            ret_list.append(&mut IntrusiveLinkedList::from_range(
                page_begin, page_mid, class,
            ));
            unsafe { self.add_to_heap(page_mid, page_end, false) };
        }
        self.used += layout.size() * n;
        self.allocated += size * n;
        ret_list.size_check().unwrap();
        Ok(ret_list)
    }
    pub fn dealloc_list(&mut self, list: IntrusiveLinkedList, layout: Layout) {
        list.size_check().unwrap();
        let (size, class) = super::layout_info(layout);
        let n = list.len();
        // Merge free buddy lists
        let mut current_class = class;
        let mut temp_list = list;
        while current_class < self.free_list.len() {
            let stop = current_class + 1 != self.free_list.len();
            let reset_min = if current_class <= 12 { 16 } else { 0 };
            match self.free_list[current_class].maybe_collection(
                temp_list,
                current_class,
                stop,
                reset_min,
            ) {
                Some(list) => temp_list = list,
                None => break,
            }
            current_class += 1;
        }
        self.used -= layout.size() * n;
        self.allocated -= size * n;
    }
}

fn prev_power_of_two(num: usize) -> usize {
    1 << (8 * (core::mem::size_of::<usize>()) - num.leading_zeros() as usize - 1)
}