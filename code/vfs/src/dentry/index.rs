use alloc::{boxed::Box, collections::BTreeMap};
use ftl_util::{
    list::InListNode,
    sync::{rw_spin_mutex::RwSpinMutex, Spin},
};

use crate::hash_name::{AllHash, HashName};

use super::{DentryCache, DentryIndexNode};

const DENTRY_HASH_TABLE: usize = 170;

type HBNode = Box<InListNode<DentryCache, DentryIndexNode>>;
/// dentry cache 哈希索引器
pub(crate) struct DentryIndex {
    table: [RwSpinMutex<BTreeMap<AllHash, HBNode>, Spin>; DENTRY_HASH_TABLE],
}

impl DentryIndex {
    pub fn new() -> Self {
        #[allow(clippy::declare_interior_mutable_const)]
        const INIT: RwSpinMutex<BTreeMap<AllHash, HBNode>, Spin> =
            RwSpinMutex::new(BTreeMap::new());
        Self { table: [INIT; _] }
    }
    pub fn init(&mut self) {}
    pub fn get(&self, d: &HashName) -> Option<&'static DentryCache> {
        stack_trace!();
        let hash = d.all_hash();
        let n = hash.0 as usize % DENTRY_HASH_TABLE;
        let lk = self.table[n].unique_lock();
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
        v.push_prev(&mut new.index_node)
    }
    /// 被移除的项必须存在 这将由所有权机制保证
    pub fn remove(&self, d: &mut DentryCache) {
        stack_trace!();
        let hash = d.hash();
        let n = hash.0 as usize % DENTRY_HASH_TABLE;
        let mut lk = self.table[n].unique_lock();
        debug_assert!(!d.index_node.is_empty());
        let last = d.index_node.is_last();
        d.index_node.pop_self();
        if !last {
            return;
        }
        let head = lk.remove(&hash).unwrap();
        drop(lk);
        debug_assert!(head.is_empty());
        drop(head);
    }
}
