use alloc::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

use crate::{mutex::RwSpinMutex, tools::CID};

use super::bcache::Cache;

pub(crate) struct CacheIndex(RwSpinMutex<BTreeMap<CID, Weak<Cache>>>);

impl CacheIndex {
    pub fn new() -> Self {
        Self(RwSpinMutex::new(BTreeMap::new()))
    }
    pub fn get(&self, cid: CID) -> Option<Arc<Cache>> {
        self.0.shared_lock().get(&cid).and_then(|p| p.upgrade())
    }
    pub fn insert(&self, cid: CID, weak: Weak<Cache>) {
        let _ = self.0.unique_lock().insert(cid, weak);
    }
    /// 两次操作在一次加锁中完成
    pub fn may_clear_insert(&self, clear: Option<CID>, cid: CID, weak: Weak<Cache>) {
        let mut m = self.0.unique_lock();
        clear.and_then(|clear| m.remove(&clear));
        let _ = m.insert(cid, weak);
    }
    /// 需要保证此块存在
    pub fn clear(&self, clear: CID) {
        let _ = self.0.unique_lock().remove(&clear).unwrap();
    }
}
