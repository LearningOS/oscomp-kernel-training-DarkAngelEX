use core::ptr::NonNull;

use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc};
use ftl_util::{
    error::{SysError, SysR},
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{
    dentry::{manager::DentryManager, Dentry, InodeS},
    fssp::{FsType, Fssp, FsspOwn},
    inode::FsInode,
    mount::{manager::MountManager, Mount},
    tmpfs::TmpFsInfo,
    VfsFile, PRINT_OP,
};

use self::path::Path;

pub mod path;

pub trait FsManager {
    fn root(&self) -> Arc<dyn FsInode>;
}

pub struct VfsManager {
    fstypes: SpinMutex<BTreeMap<String, Box<dyn FsType>>, Spin>,
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
            fstypes: SpinMutex::new(BTreeMap::new()),
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
        m.import_fstype(TmpFsInfo::new()); // 导入 tmpfs
        m
    }
    fn mounts_ptr(&self) -> NonNull<MountManager> {
        NonNull::new(&self.mounts as *const _ as *mut _).unwrap()
    }
    pub fn import_fstype(&self, fstype: Box<dyn FsType>) {
        let name = fstype.name();
        let _ = self.fstypes.lock().insert(name, fstype);
    }
    /// 初始化根目录
    fn init_root(&mut self) {
        stack_trace!();
        let root = Dentry::new_vfs_root(&self.dentrys, NonNull::new(&mut *self.root_fssp).unwrap());
        self.root = Some(root);
    }
    pub async fn open(&self, path: (impl BaseFn, &str)) -> SysR<Arc<VfsFile>> {
        stack_trace!();
        if PRINT_OP {
            println!("open: {}", path.1);
        }
        let (path, name) = self.walk_path(path).await?;
        let path = self.walk_name(path, name).await?;
        VfsFile::from_path_arc(path)
    }
    pub async fn create(&self, path: (impl BaseFn, &str), dir: bool) -> SysR<Arc<VfsFile>> {
        stack_trace!();
        if PRINT_OP {
            println!("create: {}", path.1);
        }
        let (path, name) = self.walk_path(path).await?;
        if !path.dentry.is_dir() || path::name_invalid(name) {
            return Err(SysError::ENOTDIR);
        }
        if let Ok(p) = self.walk_name(path.clone(), name).await {
            if dir || p.dentry.is_dir() {
                return Err(SysError::EEXIST);
            }
            match p.inode_s() {
                InodeS::Init => return Err(SysError::EBUSY),
                InodeS::Some(inode) => {
                    inode.reset_data().await?;
                    return VfsFile::from_path_arc(p);
                }
                InodeS::None | InodeS::Closed => (), // dentry has unlink
            }
        }
        let dentry = path.dentry.create(name, dir, (true, true)).await?;
        VfsFile::from_path_arc(Path {
            mount: path.mount,
            dentry,
        })
    }
    /// 只能unlink文件, 不能删除目录
    pub async fn unlink(&self, path: (impl BaseFn, &str)) -> SysR<()> {
        stack_trace!();
        if PRINT_OP {
            println!("unlink: {}", path.1);
        }
        let (path, name) = self.walk_path(path).await?;
        if !path.dentry.is_dir() || path::name_invalid(name) {
            return Err(SysError::ENOTDIR);
        }
        path.dentry.unlink(name).await
    }
    pub async fn rmdir(&self, path: (impl BaseFn, &str)) -> SysR<()> {
        stack_trace!();
        if PRINT_OP {
            println!("rmdir: {}", path.1);
        }
        let (path, name) = self.walk_path(path).await?;
        if !path.dentry.is_dir() || path::name_invalid(name) {
            return Err(SysError::ENOTDIR);
        }
        path.dentry.rmdir(name).await
    }
    pub async fn rename(&self, old: (impl BaseFn, &str), new: (impl BaseFn, &str)) -> SysR<()> {
        stack_trace!();
        if PRINT_OP {
            println!("rename: {} -> {}", old.1, new.1);
        }
        todo!()
    }
    pub async fn mount(
        &self,
        src: (impl BaseFn, &str),
        dir: (impl BaseFn, &str),
        fstype: &str,
        flags: usize,
    ) -> SysR<()> {
        let dir = self.walk_all(dir).await?;
        if !dir.dentry.is_dir() {
            return Err(SysError::ENOTDIR);
        }
        let mut fs = self
            .fstypes
            .lock()
            .get(fstype)
            .ok_or(SysError::EINVAL)?
            .new_fs();
        let src = if fs.need_src() {
            let src = self.walk_all(src).await?;
            Some(VfsFile::from_path(src)?)
        } else {
            None
        };
        fs.init(src, flags).await?;
        let fssp = Fssp::new(Some(fs));
        let root_inode = fssp.root_inode();
        let fssp = fssp.into_raw();
        let root = Dentry::new_root(&self.dentrys, fssp, InodeS::Some(root_inode));
        self.mount_impl(dir, root, FsspOwn::new(fssp).unwrap());
        Ok(())
    }
    pub async fn umount(&self, _dir: (impl BaseFn, &str), _flags: usize) -> SysR<()> {
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
