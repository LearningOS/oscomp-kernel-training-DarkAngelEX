use ftl_util::{container::lru::LRUManager, list::InListNode};

use super::{DentryCache, DentryLruNode};

type LRUNode = InListNode<DentryCache, DentryLruNode>;

pub struct LRUQueue(LRUManager<DentryCache, DentryLruNode>);

impl LRUQueue {
    pub fn new(max: usize) -> Self {
        Self(LRUManager::new(max))
    }
    pub(super) fn insert(&self, node: &mut LRUNode) {
        self.0
            .insert(node, |x| unsafe {
                x.access_mut().close_by_lru_0();
            })
            .map(|mut p| unsafe {
                p.as_mut().access_mut().close_by_lru_1();
            });
    }
}
