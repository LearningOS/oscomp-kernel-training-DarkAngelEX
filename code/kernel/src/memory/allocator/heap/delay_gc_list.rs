use core::ptr::NonNull;

use crate::tools::container::intrusive_linked_list::IntrusiveLinkedList;

pub(super) struct DelayGCList {
    list: IntrusiveLinkedList,
    collection_size: usize,
    run_count: usize,
}

impl DelayGCList {
    pub const fn new() -> Self {
        Self {
            list: IntrusiveLinkedList::new(),
            collection_size: 0,
            run_count: 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    pub fn len(&self) -> usize {
        self.list.len()
    }
    pub fn get_list(&mut self) -> &mut IntrusiveLinkedList {
        &mut self.list
    }
    pub unsafe fn push(&mut self, ptr: NonNull<usize>) {
        self.list.push(ptr);
        self.run_count += 1;
    }
    pub fn pop(&mut self) -> Option<NonNull<usize>> {
        self.list.pop()
    }
    pub fn collection_force(&mut self, align: usize) -> Option<IntrusiveLinkedList> {
        let list = self.list.collection(align);
        self.collection_size = self.list.len() * 2;
        list.empty_forward()
    }
    pub fn should_colloection(&self) -> bool {
        self.list.len() >= self.collection_size || self.run_count >= self.collection_size
    }
    pub fn collection_reset(&mut self, reset_min: usize) {
        self.collection_size = reset_min.max(self.list.len() * 2);
        self.run_count = 0;
    }
    pub fn maybe_collection(
        &mut self,
        mut src: IntrusiveLinkedList,
        align_log2: usize,
        stop: bool,
        reset_min: usize,
    ) -> Option<IntrusiveLinkedList> {
        self.list.append(&mut src);
        if stop || self.should_colloection() {
            return None;
        }
        // println!("\x1b[31mcollection of align {} size {}\x1b[0m", align, self.list.len());
        let list = self.list.collection(align_log2);
        self.collection_reset(reset_min);
        list.empty_forward()
    }
}
