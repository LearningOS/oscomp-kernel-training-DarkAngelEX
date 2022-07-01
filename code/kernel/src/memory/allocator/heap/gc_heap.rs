use core::{alloc::Layout, cmp::min, mem::size_of, ptr::NonNull};

use crate::tools::container::intrusive_linked_list::IntrusiveLinkedList;

use super::delay_gc_list::DelayGCList;

const SORT_BUFFER_SIZE: usize = 512;
type SortBuffer = [usize; SORT_BUFFER_SIZE];

pub struct DelayGCHeap {
    free_list: [DelayGCList; 32],
    allocated: usize, // 分配器分配的内存大小 比used更大
    total: usize,     // 分配器管理的全部内存
    sort_buffer: *mut SortBuffer,
}

unsafe impl Send for DelayGCHeap {}
unsafe impl Sync for DelayGCHeap {}

impl DelayGCHeap {
    pub const fn empty() -> Self {
        const LIST: DelayGCList = DelayGCList::new();
        Self {
            free_list: [LIST; 32],
            allocated: 0,
            total: 0,
            sort_buffer: core::ptr::null_mut(),
        }
    }
    // (used, allocated, total)
    pub fn info(&self) -> (usize, usize) {
        (self.allocated, self.total)
    }
    pub fn init(&mut self, start: usize, size: usize) {
        unsafe {
            assert!(self.sort_buffer.is_null());
            assert!(size >= SORT_BUFFER_SIZE);
            self.sort_buffer = start as *mut SortBuffer;
            self.add_to_heap(
                start + SORT_BUFFER_SIZE,
                start + size - SORT_BUFFER_SIZE,
                true,
            );
        }
    }
    /// Add a range of memory [start, end) to the heap
    pub unsafe fn add_to_heap(&mut self, mut start: usize, mut end: usize, modify_total: bool) {
        // avoid unaligned access on some platforms
        const ALIGN_SIZE: usize = size_of::<usize>();

        start = (start + ALIGN_SIZE - 1) & (!ALIGN_SIZE + 1);
        end &= !ALIGN_SIZE + 1;
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
                    let block = self.free_list[j].pop().unwrap();
                    unsafe {
                        self.free_list[j - 1].push(NonNull::new_unchecked(
                            (block.as_ptr() as usize + (1 << (j - 1))) as *mut usize,
                        ));
                        self.free_list[j - 1].push(block);
                    }
                }
                let result = self.free_list[class].pop().unwrap();
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
            ret_list.append(self.free_list[class].get_list());
            for xclass in class + 1..=take_all_class {
                let list = self.free_list[xclass].get_list();
                let mut max_len = list.len();
                while let Some(a) = list.pop() {
                    assert!(max_len != 0);
                    max_len -= 1;
                    let begin = a.as_ptr() as usize;
                    let end = begin + (1 << xclass);
                    ret_list.append(&mut IntrusiveLinkedList::from_range(begin, end, class));
                }
            }
        }
        if xn != 0 {
            let list = self.free_list[cur_class].get_list();
            while xn >= 1 << (cur_class - class) {
                let a = list.pop().unwrap();
                let begin = a.as_ptr() as usize;
                let end = begin + (1 << cur_class);
                ret_list.append(&mut IntrusiveLinkedList::from_range(begin, end, class));
                xn -= 1 << (cur_class - class);
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
        self.allocated += size * n;
        ret_list.size_check().unwrap();
        Ok(ret_list)
    }
    pub fn dealloc_list(&mut self, list: IntrusiveLinkedList, layout: Layout) {
        let (size, class) = super::layout_info(layout);
        list.size_check().unwrap();
        let n = list.len();
        // Merge free buddy lists
        let mut current_class = class;
        let mut temp_list = list;
        while current_class < self.free_list.len() {
            let stop = current_class + 1 == self.free_list.len();
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
        self.allocated -= size * n;
    }
}

/// 0 -> 1 << 63
/// 1 -> 1
/// [2,3] -> 2
/// [4,7] -> 4
fn prev_power_of_two(num: usize) -> usize {
    1 << (usize::BITS - num.leading_zeros() - 1)
}
