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
    pub fn maybe_merge(
        &mut self,
        mut src: IntrusiveLinkedList,
        align: usize,
        stop: bool,
    ) -> Option<IntrusiveLinkedList> {
        self.list.append(&mut src);
        if stop || self.list.len() < self.collection_size {
            return None;
        }
        let mask = 1 << align;
        self.list.sort();
        let mut node_iter = self.list.node_iter();
        let mut list = IntrusiveLinkedList::new();
        while let Some((a, b)) = node_iter.current_and_next() {
            if (a.as_ptr() as usize ^ b.as_ptr() as usize) == mask {
                unsafe { list.push(node_iter.remove_current_and_next()) };
                continue;
            }
            if node_iter.next().is_err() {
                break;
            }
        }
        self.list.size_reset(self.list.len() - list.len() * 2);
        self.collection_size = self.list.len() * 2;
        Some(list)
    }
}
