use core::ops::DerefMut;

use alloc::{boxed::Box, collections::BTreeMap};
use ftl_util::{
    list::InListNode,
    sync::{rw_spin_mutex::RwSpinMutex, Spin},
};

use crate::hash_name::{AllHash, HashName};

use super::{DentryCache, DentryHashNode};

const DENTRY_HASH_TABLE: usize = 170;

/// dentry cache 哈希索引器
pub struct DentryIndex {
    table: [RwSpinMutex<BTreeMap<AllHash, Box<InListNode<DentryCache, DentryHashNode>>>, Spin>;
        DENTRY_HASH_TABLE],
}

impl DentryIndex {
    pub fn new() -> Self {
        const INIT: RwSpinMutex<
            BTreeMap<AllHash, Box<InListNode<DentryCache, DentryHashNode>>>,
            Spin,
        > = RwSpinMutex::new(BTreeMap::new());
        Self { table: [INIT; _] }
    }
    pub fn get(&self, d: &HashName) -> Option<&'static DentryCache> {
        stack_trace!();
        let hash = d.all_hash();
        let n = hash.0 as usize % DENTRY_HASH_TABLE;
        let mut lk = self.table[n].unique_lock();
        let v = lk.get(&hash)?;
        v.next_iter().find(|a| a.name.all_same(d))
    }
    /// 禁止重复项被二次插入
    pub fn insert(&self, new: &mut DentryCache) {
        stack_trace!();
        let hash = new.hash();
        let n = hash.0 as usize % DENTRY_HASH_TABLE;
        let mut lk = self.table[n].unique_lock();
        let v = lk.entry(hash).or_insert_with(|| {
            let mut ptr = Box::new(InListNode::new());
            ptr.init();
            ptr
        });
        if v.next_iter().any(|a| core::ptr::eq(a, new)) {
            panic!();
        }
        v.push_prev(&mut new.hash_node)
    }
    pub fn remove(&self, d: &mut DentryCache) {
        stack_trace!();
        let hash = d.hash();
        let n = hash.0 as usize % DENTRY_HASH_TABLE;
        let mut lk = self.table[n].unique_lock();
        let last = d.hash_node.is_last();
        d.hash_node.pop_self();
        if !last {
            return;
        }
        let head = lk.remove(&hash).unwrap();
        drop(lk);
        debug_assert!(head.is_empty());
        drop(head);
    }
    pub fn lock(&self, d: &HashName) -> impl DerefMut + '_ {
        let hash = d.all_hash();
        let n = hash.0 as usize % DENTRY_HASH_TABLE;
        self.table[n].unique_lock()
    }
}
