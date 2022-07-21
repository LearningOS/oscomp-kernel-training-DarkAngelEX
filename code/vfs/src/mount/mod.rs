//! 挂载点所有权被挂载管理器持有
//!
//! 和Linux不同, 这里的mount同时兼具Linux的超级块职能
//!

pub mod manager;

use core::{
    cell::SyncUnsafeCell,
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{
    list::InListNode,
    rcu::RcuWraper,
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{dentry::Dentry, fssp::FsspOwn};

use self::manager::MountManager;

inlist_access!(MountParentNode, Mount, parent_node);
inlist_access!(pub MonutManagerNode, Mount, manager_node);

/// 一个挂载点, 使用RCU释放内存, 但在释放之前必须手动关闭
pub(crate) struct Mount {
    own: RcuWraper<Option<Box<Mount>>>, // 指向自身, 通过RCU释放
    closed: AtomicBool,
    /// 此挂载点所在的目录项
    pub locate: SyncUnsafeCell<Option<Arc<Dentry>>>,
    /// 挂点文件系统根目录 由它管理
    pub root: SyncUnsafeCell<Option<Arc<Dentry>>>,
    /// 挂载点所在目录的文件系统的挂载点, 用来保证路径的回退
    pub parent: Option<NonNull<Mount>>,
    children: SpinMutex<InListNode<Self, MountParentNode>, Spin>,
    parent_node: InListNode<Self, MountParentNode>,
    /// 全局挂载管理器
    manager: NonNull<MountManager>,
    manager_node: InListNode<Self, MonutManagerNode>,
    /// 此挂载点包含的文件系统
    fssp: FsspOwn,
}

impl Drop for Mount {
    fn drop(&mut self) {
        debug_assert!(self.closed.load(Ordering::Relaxed));
    }
}

impl Mount {
    pub fn new(
        locate: Arc<Dentry>,
        root: Arc<Dentry>,
        parent: Option<NonNull<Mount>>,
        manager: NonNull<MountManager>,
        fssp: FsspOwn,
    ) -> NonNull<Self> {
        let ptr = Box::new(Self {
            own: RcuWraper::new(None),
            closed: AtomicBool::new(false),
            locate: SyncUnsafeCell::new(Some(locate)),
            root: SyncUnsafeCell::new(Some(root)),
            parent,
            children: SpinMutex::new(InListNode::new()),
            parent_node: InListNode::new(),
            manager,
            manager_node: InListNode::new(),
            fssp,
        });
        let raw = Box::into_raw(ptr);
        unsafe {
            let this = &mut *raw;
            *this.own.get_mut() = Some(Box::from_raw(raw));
            this.children.get_mut().init();
            this.parent_node.init();
            this.manager_node.init();
            if let Some(mut parent) = this.parent {
                parent
                    .as_mut()
                    .children
                    .lock()
                    .push_prev(&mut this.parent_node);
            }
            let ptr = NonNull::new(raw).unwrap();
            this.locate().cache.mount.rcu_write(Some(ptr));
            this.manager.as_mut().insert_mount(&mut this.manager_node);
            ptr
        }
    }
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub unsafe fn locate(&self) -> &Dentry {
        (*self.locate.get()).as_ref().unwrap()
    }
    pub unsafe fn locate_arc(&self) -> Arc<Dentry> {
        (*self.locate.get()).as_ref().unwrap().clone()
    }
    pub unsafe fn root(&self) -> &Dentry {
        (*self.root.get()).as_ref().unwrap()
    }
    pub unsafe fn root_arc(&self) -> Arc<Dentry> {
        (*self.root.get()).as_ref().unwrap().clone()
    }
    /// 此函数调用之前需要检查并上锁
    ///
    /// 只有释放了所有资源并通过close禁用访问才可以关闭
    pub unsafe fn close_impl(&mut self) {
        debug_assert!(self.closed());
        debug_assert!(self.children.get_mut().is_empty());
        self.locate().cache.mount.rcu_write(None);
        *self.locate.get_mut() = None;
        *self.root.get_mut() = None;
        if let Some(mut p) = self.parent {
            let _lk = p.as_mut().children.lock();
            self.parent_node.pop_self();
        }
        self.manager.as_ref().remove_mount(&mut self.manager_node);
        if self.fssp.drop() {
            // 释放文件系统
            todo!()
        }
    }
}
