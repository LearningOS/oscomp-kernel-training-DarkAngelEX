use core::ptr::NonNull;

use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc};
use ftl_util::error::SysR;

use crate::{
    dentry::{manager::DentryManager, Dentry},
    fssp::{Fs, Fssp, FsspOwn},
    inode::FsInode,
    mount::{manager::MountManager, Mount},
    tmpfs::TmpFs,
    VfsFile,
};

use self::path::Path;

pub mod path;

pub trait FsManager {
    fn root(&self) -> Arc<dyn FsInode>;
}

pub struct VfsManager {
    root: Option<Arc<Dentry>>,
    root_fssp: Box<Fssp>,
    special_dentry: BTreeMap<String, Arc<Dentry>>,
    dentrys: DentryManager,
    mounts: MountManager,
}

pub trait BaseFn = FnOnce() -> SysR<Arc<VfsFile>>;

impl VfsManager {
    /// max: 最大缓存数量
    pub fn new(max: usize) -> Box<Self> {
        let mut m = Box::new(Self {
            root: None,
            root_fssp: Fssp::new(None),
            special_dentry: BTreeMap::new(),
            dentrys: DentryManager::new(max),
            mounts: MountManager::new(),
        });
        m.root_fssp.rc_increase();
        m.dentrys.init();
        m.mounts.init();
        m.init_root();
        m
    }
    fn mounts_ptr(&self) -> NonNull<MountManager> {
        NonNull::new(&self.mounts as *const _ as *mut _).unwrap()
    }
    /// 初始化根目录
    fn init_root(&mut self) {
        let root = Dentry::new_vfs_root(&self.dentrys, NonNull::new(&mut *self.root_fssp).unwrap());
        self.root = Some(root);
    }
    pub async fn open(&self, path: (impl BaseFn, &str)) -> SysR<VfsFile> {
        let (path, name) = self.walk_path(path).await?;
        let path = self.walk_name(path, name).await?;
        VfsFile::from_path(path)
    }
    pub async fn create(&self, path: (impl BaseFn, &str), dir: bool) -> SysR<VfsFile> {
        let (path, name) = self.walk_path(path).await?;
        todo!()
    }
    /// 只能unlink文件, 不能删除目录
    pub async fn unlink(&self, path: (impl BaseFn, &str)) -> SysR<()> {
        let (path, name) = self.walk_path(path).await?;
        todo!()
    }
    pub async fn rmdir(&self, path: (impl BaseFn, &str)) -> SysR<()> {
        let (path, name) = self.walk_path(path).await?;
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
        let dir = self.wake_all(dir).await?;
        let fs: Box<dyn Fs> = match fstype {
            "tmpfs" => TmpFs::new(),
            _ => panic!(),
        };
        let fssp = Fssp::new(Some(fs));
        let root_inode = fssp.root_inode();
        let fssp = fssp.into_raw();
        let root = Dentry::new_root(&self.dentrys, fssp, Some(root_inode));
        self.mount_impl(dir, root, FsspOwn::new(fssp).unwrap());
        todo!()
    }
    pub async fn umount(&self, dir: (impl BaseFn, &str), flags: usize) -> SysR<()> {
        todo!()
    }
    fn mount_impl(
        &self,
        Path {
            mount: parent,
            dentry: locate,
        }: Path,
        root: Arc<Dentry>,
        fssp: FsspOwn,
    ) {
        let _mount = Mount::new(locate, root, parent, self.mounts_ptr(), fssp);
    }
}
