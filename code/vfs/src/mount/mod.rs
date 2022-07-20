//! 挂载点所有权被挂载管理器持有
//!
//! 和Linux不同, 这里的mount同时兼具Linux的超级块职能
//!

pub mod manager;

use core::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{
    list::InListNode,
    rcu::RcuWraper,
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{dentry::Dentry, spfs::Fssp};

use self::manager::MountManager;

inlist_access!(MountParentNode, Mount, parent_node);
inlist_access!(pub MountFsspNode, Mount, fssp_node);
inlist_access!(MonutManagerNode, Mount, manager_node);

/// 一个挂载点, 使用RCU释放内存, 但在释放之前必须手动关闭
pub struct Mount {
    own: RcuWraper<Option<Box<Mount>>>, // 指向自身, 通过RCU释放
    closed: AtomicBool,
    /// 此挂载点所在的目录项
    dentry: Arc<Dentry>,
    /// 挂载点所在目录的文件系统的挂载点
    parent: Option<NonNull<Mount>>,
    children: SpinMutex<InListNode<Self, MountParentNode>, Spin>,
    parent_node: InListNode<Self, MountParentNode>,
    /// 全局挂载管理器
    manager: NonNull<MountManager>,
    manager_node: InListNode<Self, MonutManagerNode>,
    /// 此挂载点包含的文件系统
    fssp: NonNull<Fssp>,
    fssp_node: InListNode<Self, MountFsspNode>,
}

impl Drop for Mount {
    fn drop(&mut self) {
        debug_assert!(self.closed.load(Ordering::Relaxed));
    }
}

impl Mount {
    pub fn new(
        dentry: Arc<Dentry>,
        parent: Option<NonNull<Mount>>,
        manager: NonNull<MountManager>,
        fssp: NonNull<Fssp>,
    ) -> NonNull<Self> {
        let ptr = Box::new(Self {
            own: RcuWraper::new(None),
            closed: AtomicBool::new(false),
            dentry,
            parent,
            children: SpinMutex::new(InListNode::new()),
            parent_node: InListNode::new(),
            manager,
            manager_node: InListNode::new(),
            fssp,
            fssp_node: InListNode::new(),
        });
        let raw = Box::into_raw(ptr);
        unsafe {
            let this = &mut *raw;
            *this.own.get_mut() = Some(Box::from_raw(raw));
            this.children.get_mut().init();
            this.parent_node.init();
            this.fssp_node.init();
            this.manager_node.init();
            if let Some(mut parent) = this.parent {
                parent
                    .as_mut()
                    .children
                    .lock()
                    .push_prev(&mut this.parent_node);
            }
            this.manager.as_mut().insert_mount(&mut this.manager_node);
            this.fssp.as_mut().insert_mount(&mut this.fssp_node);
        }
        NonNull::new(raw).unwrap()
    }
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub fn close(&self) {
        debug_assert!(!self.closed());
        self.closed.store(true, Ordering::Release);
    }
}
