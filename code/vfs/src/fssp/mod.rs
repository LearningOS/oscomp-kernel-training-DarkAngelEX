//! special fs

use core::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{boxed::Box, string::String, sync::Arc};
use ftl_util::{
    async_tools::ASysR,
    list::InListNode,
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{
    dentry::{DentryCache, DentryFsspNode},
    inode::{FsInode, InodeFsspNode, VfsInode},
    manager::VfsSpawner,
    VfsFile,
};

/// 用来注册一个文件系统
pub trait FsType: Send + Sync + 'static {
    fn name(&self) -> String;
    fn new_fs(&self) -> Box<dyn Fs>;
}

pub trait Fs: Send + Sync + 'static {
    fn need_src(&self) -> bool;
    fn need_spawner(&self) -> bool;
    fn init(&mut self, file: Option<VfsFile>, flags: usize) -> ASysR<()>;
    fn set_spawner(&mut self, spawner: Box<dyn VfsSpawner>) -> ASysR<()>;
    fn root(&self) -> Box<dyn FsInode>;
}

pub(crate) struct FsspOwn(Option<NonNull<Fssp>>);

impl FsspOwn {
    pub fn new(p: NonNull<Fssp>) -> Option<Self> {
        unsafe { p.as_ref().rc_increase().then_some(Self(Some(p))) }
    }
    pub fn drop(&mut self) -> bool {
        unsafe { self.0.take().unwrap().as_ref().rc_decrease() }
    }
    /// 此函数不需要锁
    pub fn clone(&self) -> Option<Self> {
        Self::new(self.0?)
    }
}

/// fs special
pub(crate) struct Fssp {
    /// 引用计数: -1: init 0: closed other: using
    rc: AtomicUsize,
    fs: Option<Box<dyn Fs>>,
    /// 此文件系统上未使用的DentryCache
    dentrys: SpinMutex<InListNode<DentryCache, DentryFsspNode>, Spin>,
    /// 此文件系统上缓存的inode
    inodes: SpinMutex<InListNode<VfsInode, InodeFsspNode>, Spin>,
}

impl Fssp {
    pub fn new(fs: Option<Box<dyn Fs>>) -> Box<Self> {
        let mut ptr = Box::new(Self {
            rc: AtomicUsize::new(usize::MAX),
            dentrys: SpinMutex::new(InListNode::new()),
            inodes: SpinMutex::new(InListNode::new()),
            fs,
        });
        ptr.dentrys.get_mut().init();
        ptr.inodes.get_mut().init();
        ptr
    }
    pub fn into_raw(self: Box<Self>) -> NonNull<Self> {
        NonNull::new(Box::into_raw(self)).unwrap()
    }
    pub fn get_raw(&self) -> NonNull<Self> {
        NonNull::new(self as *const _ as *mut Self).unwrap()
    }
    /// 返回递增是否成功
    pub fn rc_increase(&self) -> bool {
        let mut cur = self.rc.load(Ordering::Relaxed);
        loop {
            if cur == 0 {
                return false;
            }
            let new = match cur {
                usize::MAX => 1,
                _ => cur + 1,
            };
            match self
                .rc
                .compare_exchange(cur, new, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return true,
                Err(v) => cur = v,
            }
        }
    }
    /// 当递减到0时返回true
    pub fn rc_decrease(&self) -> bool {
        if cfg!(debug_assertions) {
            let cur = self.rc.load(Ordering::Relaxed);
            assert!(cur != 0 && cur != usize::MAX);
        }
        let v = self.rc.fetch_sub(1, Ordering::Release);
        debug_assert!(v != usize::MAX);
        v == 1
    }
    pub fn insert_dentry<T>(
        &self,
        new: &mut InListNode<DentryCache, DentryFsspNode>,
        locked_run: impl FnOnce() -> T,
    ) -> T {
        let mut lk = self.dentrys.lock();
        lk.push_prev(new);
        locked_run()
    }
    pub fn remove_dentry(&self, d: &mut InListNode<DentryCache, DentryFsspNode>) {
        let _lk = self.dentrys.lock();
        debug_assert!(!d.is_empty());
        d.pop_self();
    }
    pub fn root_inode(&self) -> Arc<VfsInode> {
        VfsInode::new(self.get_raw(), self.fs.as_ref().unwrap().root())
    }
}
