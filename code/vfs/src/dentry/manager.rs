use core::ptr::NonNull;

use super::{index::DentryIndex, lru_queue::LRUQueue};

pub(crate) struct DentryManager {
    pub index: DentryIndex,
    pub lru: LRUQueue,
}

impl DentryManager {
    pub fn new(max: usize) -> Self {
        Self {
            index: DentryIndex::new(),
            lru: LRUQueue::new(max),
        }
    }
    pub fn init(&mut self) {
        self.index.init();
        self.lru.init();
    }
    pub fn lru_ptr(&self) -> NonNull<LRUQueue> {
        NonNull::new(&self.lru as *const _ as *mut _).unwrap()
    }
    pub fn index_ptr(&self) -> NonNull<DentryIndex> {
        NonNull::new(&self.index as *const _ as *mut _).unwrap()
    }
}
