use core::ptr::NonNull;

use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc};
use ftl_util::{
    async_tools::Async,
    error::{SysError, SysR},
    sync::{spin_mutex::SpinMutex, Spin},
    time::Instant,
};

use crate::{
    dentry::{manager::DentryManager, Dentry, DentryCache, InodeS},
    fssp::{FsType, Fssp, FsspOwn},
    hash_name::HashName,
    inode::VfsInode,
    mount::{manager::MountManager, Mount},
    tmpfs::{TmpFs, TmpFsType},
    FsInode, VfsFile, PRINT_OP,
};

use self::path::Path;

pub mod path;

/// 用来给文件系统生成同步线程
pub trait VfsSpawner: Send + Sync + 'static {
    fn box_clone(&self) -> Box<dyn VfsSpawner>;
    fn spawn(&self, future: Async<'static, ()>);
}
pub trait VfsClock: Send + Sync + 'static {
    fn box_clone(&self) -> Box<dyn VfsClock>;
    fn now(&self) -> Instant;
}
pub struct ZeroClock;
impl VfsClock for ZeroClock {
    fn box_clone(&self) -> Box<dyn VfsClock> {
        Box::new(ZeroClock)
    }
    fn now(&self) -> Instant {
        Instant::BASE
    }
}

pub struct VfsManager {
    fstypes: SpinMutex<BTreeMap<String, Box<dyn FsType>>, Spin>,
    root: Option<Arc<Dentry>>,
    root_fssp: Box<Fssp>,
    special_dir: BTreeMap<String, Arc<Dentry>>, // 特殊文件会挂载到根目录
    dentrys: DentryManager,
    mounts: MountManager,
    spawner: Option<Box<dyn VfsSpawner>>,
    clock: Option<Box<dyn VfsClock>>,
}

pub trait BaseFn = FnOnce() -> SysR<Arc<VfsFile>>;

impl VfsManager {
    /// max: 最大缓存数量
    pub fn new(max: usize) -> Box<Self> {
        let mut m = Box::new(Self {
            fstypes: SpinMutex::new(BTreeMap::new()),
            root: None,
            root_fssp: Fssp::new(None),
            special_dir: BTreeMap::new(),
            dentrys: DentryManager::new(max),
            mounts: MountManager::new(),
            spawner: None,
            clock: None,
        });
        m.root_fssp.rc_increase();
        m.dentrys.init();
        m.mounts.init();
        m.init_root();
        m.import_fstype(TmpFsType::new()); // 导入 tmpfs
        m
    }

    pub fn init_spawner(&mut self, spawner: Box<dyn VfsSpawner>) {
        self.spawner = Some(spawner);
    }
    pub fn init_clock(&mut self, clock: Box<dyn VfsClock>) {
        self.clock = Some(clock);
    }
    pub fn import_fstype(&self, fstype: Box<dyn FsType>) {
        let name = fstype.name();
        let _ = self.fstypes.lock().insert(name, fstype);
    }
    /// 这里创建的目录将全局可见
    pub fn set_spec_dentry(&mut self, name: String) {
        let parent = self.root.as_ref().unwrap().clone();
        let fssp = NonNull::new(&mut *self.root_fssp).unwrap();
        let dentry = DentryCache::new(
            HashName::new(&*parent, &name),
            true,
            Some(parent),
            InodeS::Some(VfsInode::new(fssp, TmpFs::new_dir())),
            NonNull::new(&mut self.dentrys.lru).unwrap(),
            fssp,
            NonNull::new(&mut self.dentrys.index).unwrap(),
            false,
        );
        self.special_dir.try_insert(name, dentry).ok().unwrap();
    }
    /// 初始化根目录
    fn init_root(&mut self) {
        stack_trace!();
        let root = Dentry::new_vfs_root(&self.dentrys, NonNull::new(&mut *self.root_fssp).unwrap());
        self.root = Some(root);
    }
    fn mounts_ptr(&self) -> NonNull<MountManager> {
        NonNull::new(&self.mounts as *const _ as *mut _).unwrap()
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
    pub async fn place_inode(
        &self,
        path: (impl BaseFn, &str),
        inode: Box<dyn FsInode>,
    ) -> SysR<Arc<VfsFile>> {
        stack_trace!();
        if PRINT_OP {
            println!("set_inode: {}", path.1);
        }
        if inode.is_dir() {
            println!("try set dir inode!");
            return Err(SysError::EISDIR);
        }
        let (path, name) = self.walk_path(path).await?;
        if !path.dentry.is_dir() || path::name_invalid(name) {
            return Err(SysError::ENOTDIR);
        }
        if let Ok(_) = self.walk_name(path.clone(), name).await {
            return Err(SysError::EEXIST);
        }
        let dentry = path.dentry.place_inode(name, inode).await?;
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

        let src = match fs.need_src() {
            true => Some(VfsFile::from_path_arc(self.walk_all(src).await?)?),
            false => None,
        };
        fs.init(src, flags, self.clock.as_ref().unwrap().box_clone())
            .await?;
        if fs.need_spawner() {
            let spawner = self.spawner.as_ref().unwrap().box_clone();
            fs.set_spawner(spawner).await?;
        }
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
