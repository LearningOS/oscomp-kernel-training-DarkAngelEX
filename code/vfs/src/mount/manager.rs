use ftl_util::{
    list::InListNode,
    sync::{spin_mutex::SpinMutex, Spin},
};

use super::{MonutManagerNode, Mount};

/// 管理全局挂载点和文件系统, 持有每个挂载点的所有权
///
/// 索引挂载点: 当前dentry
pub struct MountManager {
    mounts: SpinMutex<InListNode<Mount, MonutManagerNode>, Spin>,
}

impl MountManager {
    pub fn new() -> Self {
        Self {
            mounts: SpinMutex::new(InListNode::new()),
        }
    }
    pub fn init(&mut self) {
        self.mounts.get_mut().init();
    }
    pub(super) fn insert_mount(&self, new: &mut InListNode<Mount, MonutManagerNode>) {
        self.mounts.lock().push_prev(new)
    }
    pub(super) fn remove_mount(&self, m: &mut InListNode<Mount, MonutManagerNode>) {
        let _lk = self.mounts.lock();
        m.pop_self();
    }
}
