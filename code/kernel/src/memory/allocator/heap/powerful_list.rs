use core::ptr::NonNull;

use crate::tools::container::intrusive_linked_list::IntrusiveLinkedList;

pub(super) struct PowerfulList {
    list: IntrusiveLinkedList,
    collection_size: usize,
}

impl PowerfulList {
    pub const fn new() -> Self {
        Self {
            list: IntrusiveLinkedList::new(),
            collection_size: 0,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    pub unsafe fn push(&mut self, ptr: NonNull<usize>) {
        self.list.push(ptr)
    }
    pub fn pop(&mut self) -> Option<NonNull<usize>> {
        self.list.pop()
    }
    pub fn maybe_collection(
        &mut self,
        mut src: IntrusiveLinkedList,
        align: usize,
        stop: bool,
    ) -> Option<IntrusiveLinkedList> {
        self.list.append(&mut src);
        if stop || self.list.len() < self.collection_size {
            return None;
        }
        let list = self.list.collection(align);
        self.collection_size = self.list.len() * 2;
        if list.is_empty() {
            return None;
        }
        Some(list)
    }
}
