use alloc::{boxed::Box, sync::Arc};
use ftl_util::error::SysR;

use crate::{
    dentry::{manager::DentryManager, Dentry},
    inode::{FsInode, VfsInode},
    mount::manager::MountManager,
};

mod path;

pub trait FsManager {
    fn root(&self) -> Arc<dyn FsInode>;
}

pub struct VfsManager {
    root: Arc<Dentry>,
    dentrys: DentryManager,
    mounts: MountManager,
}

pub trait BaseFn = FnOnce() -> SysR<Arc<Dentry>>;

impl VfsManager {
    pub fn new() -> Box<Self> {
        todo!()
    }
    pub async fn open(&self, path: (impl BaseFn, &str)) -> SysR<VfsInode> {
        todo!()
    }
    pub async fn create(&self, path: (impl BaseFn, &str), dir: bool) -> SysR<VfsInode> {
        todo!()
    }
    /// 只能unlink文件, 不能删除目录
    pub async fn unlink(&self, path: (impl BaseFn, &str)) -> SysR<()> {
        todo!()
    }
    pub async fn rmdir(&self, path: (impl BaseFn, &str)) -> SysR<()> {
        todo!()
    }
    pub async fn rename(&self, old: (impl BaseFn, &str), new: (impl BaseFn, &str)) -> SysR<()> {
        todo!()
    }
    pub async fn mount(
        &self,
        src: (impl BaseFn, &str),
        dir: (impl BaseFn, &str),
        fstype: &str,
        flags: usize,
    ) -> SysR<()> {
        todo!()
    }
    pub async fn umount(&self, dir: (impl BaseFn, &str), flags: usize) -> SysR<()> {
        todo!()
    }
}
