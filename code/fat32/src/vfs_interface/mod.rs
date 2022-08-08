use core::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::SysRet,
    fs::{
        stat::{Stat, S_IFDIR, S_IFREG},
        DentryType,
    },
    time::{Instant, TimeSpec},
};
use vfs::{File, Fs, FsInode, FsType, VfsClock, VfsFile, VfsSpawner};

use crate::{AnyInode, Fat32Manager};

pub struct Fat32Type;

impl FsType for Fat32Type {
    fn name(&self) -> String {
        "vfat".to_string()
    }
    fn new_fs(&self, dev: usize) -> Box<dyn Fs> {
        let list_max_dirty = 100;
        let list_max_cache = 100;
        let block_max_dirty = 100;
        let block_max_cache = 100;
        let inode_target_free = 100;
        let manager = Fat32Manager::new(
            dev,
            list_max_dirty,
            list_max_cache,
            block_max_dirty,
            block_max_cache,
            inode_target_free,
        );
        Box::new(Fat32 { manager })
    }
}
impl const Default for Fat32Type {
    fn default() -> Self {
        Self::new()
    }
}
impl Fat32Type {
    pub const fn new() -> Self {
        Self
    }
}

struct Fat32 {
    manager: Fat32Manager,
}

impl Fs for Fat32 {
    fn need_src(&self) -> bool {
        true
    }
    fn need_spawner(&self) -> bool {
        true
    }
    fn init(
        &mut self,
        file: Option<Arc<VfsFile>>,
        _flags: usize,
        clock: Box<dyn VfsClock>,
    ) -> ASysR<()> {
        Box::pin(async move {
            let device = file.unwrap().block_device()?;
            self.manager.init(device, clock).await;
            Ok(())
        })
    }
    fn set_spawner(&mut self, spawner: Box<dyn VfsSpawner>) -> ASysR<()> {
        Box::pin(async move {
            self.manager.spawn_sync_task((2, 2), spawner).await;
            Ok(())
        })
    }
    fn root(&self) -> Box<dyn FsInode> {
        let root = self.manager.root_dir();
        let manager = NonNull::new(&self.manager as *const _ as *mut Fat32Manager).unwrap();
        let rw = root.attr().rw();
        Fat32InodeV::new_dyn(AnyInode::Dir(root), rw, manager)
    }
}

struct Fat32InodeV {
    readable: AtomicBool,
    writable: AtomicBool,
    inode: AnyInode,
    manager: NonNull<Fat32Manager>,
    ino: usize,
}

unsafe impl Send for Fat32InodeV {}
unsafe impl Sync for Fat32InodeV {}

impl Fat32InodeV {
    pub fn new(inode: AnyInode, (r, w): (bool, bool), manager: NonNull<Fat32Manager>) -> Self {
        let iid = unsafe {
            match &inode {
                AnyInode::Dir(d) => d.inode.unsafe_get(),
                AnyInode::File(f) => f.inode.unsafe_get(),
            }
            .cache
            .inner
            .unsafe_get()
            .entry
            .iid(manager.as_ref())
        };
        Fat32InodeV {
            readable: AtomicBool::new(r),
            writable: AtomicBool::new(w),
            inode,
            manager,
            ino: iid.get() as usize,
        }
    }
    pub fn new_dyn(
        inode: AnyInode,
        rw: (bool, bool),
        manager: NonNull<Fat32Manager>,
    ) -> Box<dyn FsInode> {
        Box::new(Self::new(inode, rw, manager))
    }
    fn manager(&self) -> &Fat32Manager {
        unsafe { self.manager.as_ref() }
    }
}

impl FsInode for Fat32InodeV {
    fn readable(&self) -> bool {
        self.readable.load(Ordering::Acquire)
    }
    fn writable(&self) -> bool {
        self.writable.load(Ordering::Acquire)
    }
    fn is_dir(&self) -> bool {
        self.inode.dir().is_ok()
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            let dev = self.manager().dev;
            let bpb = self.manager().bpb();
            let short = self.inode.short_name();
            let size = short.file_bytes();
            let blk_size = bpb.cluster_bytes as u32;
            let blk_n = self.inode.blk_num(self.manager()).await? as u64;
            let access_time = short.access_time();
            let modify_time = short.modify_time();
            stat.st_dev = dev as u64;
            stat.st_ino = self.ino as u64;
            stat.st_mode = 0o777;
            match &self.inode {
                AnyInode::Dir(_) => stat.st_mode |= S_IFDIR,
                AnyInode::File(_) => stat.st_mode |= S_IFREG,
            }
            stat.st_nlink = 1;
            stat.st_uid = 0;
            stat.st_gid = 0;
            stat.st_rdev = 0;
            stat.st_size = size;
            stat.st_blksize = blk_size;
            stat.st_blocks = blk_n * (blk_size / 512) as u64;
            stat.st_atime = access_time.second();
            stat.st_atime_nsec = access_time.nanosecond();
            stat.st_mtime = modify_time.second();
            stat.st_mtime_nsec = access_time.nanosecond();
            stat.st_ctime = modify_time.second();
            stat.st_ctime_nsec = access_time.nanosecond();
            Ok(())
        })
    }
    fn utimensat(&self, times: [TimeSpec; 2], now: fn() -> Instant) -> ASysR<()> {
        Box::pin(async move {
            let [access, modify] = times
                .try_map(|v| v.user_map(now))?
                .map(|v| v.map(|v| v.as_instant()));
            self.inode.update_time(access, modify).await;
            Ok(())
        })
    }
    fn detach(&self) -> ASysR<()> {
        Box::pin(async move {
            match &self.inode {
                AnyInode::Dir(v) => v.detach(self.manager()).await,
                AnyInode::File(v) => v.detach(self.manager()).await,
            }
        })
    }
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        Box::pin(async move {
            let dir = self.inode.dir()?;
            dir.list(self.manager()).await
        })
    }
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move {
            let dir = self.inode.dir()?;
            let any = dir.search_any(self.manager(), name).await?;
            let rw = any.attr().rw();
            Ok(Fat32InodeV::new_dyn(any, rw, self.manager))
        })
    }
    fn create<'a>(&'a self, name: &'a str, dir: bool, rw: (bool, bool)) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move {
            let parent = self.inode.dir()?;
            let m = self.manager();
            match dir {
                true => parent.create_dir(m, name, rw.1, false).await?,
                false => parent.create_file(m, name, rw.1, false).await?,
            }
            let any = parent.search_any(m, name).await?;
            let rw = any.attr().rw();
            Ok(Fat32InodeV::new_dyn(any, rw, self.manager))
        })
    }
    fn unlink_child<'a>(&'a self, name: &'a str, release: bool) -> ASysR<()> {
        Box::pin(async move {
            if !release {
                return Ok(());
            }
            assert!(release); // 延迟释放尚未实现
            let dir = self.inode.dir()?;
            dir.delete_file(self.manager(), name, release).await?;
            Ok(())
        })
    }
    fn rmdir_child<'a>(&'a self, name: &'a str) -> ASysR<()> {
        Box::pin(async move {
            let dir = self.inode.dir()?;
            dir.delete_dir(self.manager(), name).await?;
            Ok(())
        })
    }
    fn bytes(&self) -> SysRet {
        Ok(self.inode.file()?.bytes())
    }
    fn reset_data(&self) -> ASysR<()> {
        Box::pin(async move {
            let file = self.inode.file()?;
            let mut inner = file.inode.unique_lock().await;
            inner.resize(self.manager(), 0, |_: &mut [u8]| {}).await?;
            inner.update_file_bytes(0);
            Ok(())
        })
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        (offset, ptr): (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            let inode = self.inode.file()?;
            let n = inode.read_at(self.manager(), offset, buf).await?;
            if let Some(ptr) = ptr {
                ptr.store(offset + n, Ordering::Release);
            }
            Ok(n)
        })
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        (offset, ptr): (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            let inode = self.inode.file()?;
            let n = inode.write_at(self.manager(), offset, buf).await?;
            if let Some(ptr) = ptr {
                ptr.store(offset + n, Ordering::Release);
            }
            Ok(n)
        })
    }
}
