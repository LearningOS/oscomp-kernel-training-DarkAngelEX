use core::ptr::NonNull;

use ftl_util::{container::lru::LRUManager, list::InListNode};

use super::{DentryCache, DentryLruNode};

type LRUNode = InListNode<DentryCache, DentryLruNode>;

pub(crate) struct LRUQueue(LRUManager<DentryCache, DentryLruNode>);

impl LRUQueue {
    pub fn new(max: usize) -> Self {
        Self(LRUManager::new(max))
    }
    pub fn init(&mut self) {
        self.0.init();
    }
    fn release_fn() -> (
        impl FnOnce(&mut InListNode<DentryCache, DentryLruNode>),
        impl FnOnce(NonNull<InListNode<DentryCache, DentryLruNode>>),
    ) {
        (
            |x| unsafe {
                x.access_mut().close_by_lru_0();
            },
            |mut p| unsafe {
                p.as_mut().access_mut().close_by_lru_1();
            },
        )
    }
    /// 超过数量限制将移除最后一个
    pub fn insert<T>(&self, node: &mut LRUNode, locked_run: impl FnOnce() -> T) -> T {
        let (release, then) = Self::release_fn();
        let (r, v) = self.0.insert(node, locked_run, release);
        v.map(then);
        r
    }
    pub fn remove_last(&self) {
        let (release, then) = Self::release_fn();
        self.0.remove_last(release).map(then);
    }
    pub fn try_remove(&self, node: &mut LRUNode) -> Result<(), ()> {
        let (release, then) = Self::release_fn();
        self.0.try_remove(node, release)?;
        then(NonNull::new(node).unwrap());
        Ok(())
    }
    pub fn lock_run<R>(&self, f: impl FnOnce() -> R) -> R {
        self.0.lock_run(f)
    }
}
