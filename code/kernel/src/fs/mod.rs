use core::sync::atomic::AtomicUsize;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use fat32::{vfs_interface::Fat32Type, BlockDevice};
use ftl_util::{
    async_tools::{ASysR, Async},
    error::{SysError, SysR, SysRet},
    fs::{path, stat::Stat, DentryType, Mode, OpenFlags},
    time::Instant,
};
use vfs::{File, FsInode, VfsClock, VfsFile, VfsManager, VfsSpawner};

use crate::{
    drivers, executor,
    fs::dev::{null::NullInode, tty::TtyInode, zero::ZeroInode},
    memory::user_ptr::UserInOutPtr,
    timer,
    user::AutoSie,
};

pub mod dev;
pub mod pipe;
pub mod stdio;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iovec {
    pub iov_base: UserInOutPtr<u8>,
    pub iov_len: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Pollfd {
    pub fd: u32,
    pub events: u16,
    pub revents: u16,
}

static mut VFS_MANAGER: Option<Box<VfsManager>> = None;

fn vfs_manager() -> &'static VfsManager {
    unsafe { VFS_MANAGER.as_ref().unwrap() }
}

struct SysClock;
impl VfsClock for SysClock {
    fn box_clone(&self) -> Box<dyn VfsClock> {
        Box::new(Self)
    }
    fn now(&self) -> Instant {
        timer::now()
    }
}

struct SysSpawner;
impl VfsSpawner for SysSpawner {
    fn box_clone(&self) -> Box<dyn VfsSpawner> {
        Box::new(Self)
    }
    fn spawn(&self, future: Async<'static, ()>) {
        executor::kernel_spawn(future);
    }
}

pub async fn init() {
    stack_trace!();
    let max = 100;
    let mut vfs = VfsManager::new(max);
    vfs.init_clock(Box::new(SysClock));
    vfs.init_spawner(Box::new(SysSpawner));
    vfs.import_fstype(Box::new(Fat32Type::new()));
    vfs.set_spec_dentry("dev".to_string());
    vfs.set_spec_dentry("shm".to_string());
    vfs.set_spec_dentry("etc".to_string());
    const XF: SysR<Arc<VfsFile>> = Err(SysError::ENOENT);
    vfs.mount((XF, ""), (XF, "/dev"), "tmpfs", 0).await.unwrap();
    vfs.mount((XF, ""), (XF, "/shm"), "tmpfs", 0).await.unwrap();
    vfs.mount((XF, ""), (XF, "/etc"), "tmpfs", 0).await.unwrap();
    vfs.place_inode((XF, "/dev/null"), Box::new(NullInode))
        .await
        .unwrap();
    vfs.place_inode((XF, "/dev/tty"), Box::new(TtyInode))
        .await
        .unwrap();
    vfs.place_inode((XF, "/dev/zero"), Box::new(ZeroInode))
        .await
        .unwrap();
    let device = Box::new(BlockDeviceWraper(drivers::device().clone()));
    vfs.place_inode((XF, "/dev/sda1"), device).await.unwrap();
    vfs.mount((XF, "/dev/sda1"), (XF, "/"), "vfat", 0)
        .await
        .unwrap();
    unsafe {
        VFS_MANAGER = Some(vfs);
    }
}

pub async fn open_file(
    path: (SysR<Arc<VfsFile>>, &str),
    flags: OpenFlags,
    _mode: Mode,
) -> SysR<Arc<VfsFile>> {
    // 处理各种标志位
    stack_trace!();
    let _sie = AutoSie::new();
    let rw = flags.read_write()?;
    let vfs = vfs_manager();
    if flags.create() {
        match vfs.create(path.clone(), flags.dir(), rw).await {
            Ok(_) => (),
            Err(SysError::EEXIST) => {
                if flags.dir() {
                    return Err(SysError::EISDIR);
                }
                vfs.unlink(path.clone()).await?;
                vfs.create(path.clone(), false, rw).await?;
            }
            Err(e) => return Err(e),
        }
    }
    let file = vfs.open(path).await?;
    if rw.1 && !file.writable() {
        return Err(SysError::EACCES);
    }
    Ok(file)
}

pub async fn create_any(
    path: (SysR<Arc<VfsFile>>, &str),
    flags: OpenFlags,
    _mode: Mode,
) -> SysR<Arc<VfsFile>> {
    stack_trace!();
    let dir = flags.dir();
    let rw = flags.read_write()?;
    let vfs = vfs_manager();
    vfs.create(path, dir, rw).await
}

pub async fn open_file_abs(path: &str, flags: OpenFlags, mode: Mode) -> SysR<Arc<VfsFile>> {
    stack_trace!();
    debug_assert!(path::is_absolute_path(path));
    open_file((Err(SysError::ENOENT), path), flags, mode).await
}

pub async fn unlink(path: (SysR<Arc<VfsFile>>, &str), flags: OpenFlags) -> SysR<()> {
    stack_trace!();
    debug_assert!(!flags.dir());
    let vfs = vfs_manager();
    vfs.unlink(path).await
}

/// 显示根目录的东西
pub async fn list_apps() {
    stack_trace!();
    println!("/**** APPS ****");
    let vfs = vfs_manager();
    for (dt, name) in vfs.root().list().await.unwrap() {
        println!("{} {:?}", name, dt);
    }
    println!("**************/");
}

struct BlockDeviceWraper(Arc<dyn BlockDevice>);

impl FsInode for BlockDeviceWraper {
    fn block_device(&self) -> SysR<Arc<dyn BlockDevice>> {
        Ok(self.0.clone())
    }
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        true
    }
    fn is_dir(&self) -> bool {
        false
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        todo!()
    }
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        todo!()
    }
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>> {
        todo!()
    }
    fn create<'a>(&'a self, name: &'a str, dir: bool, rw: (bool, bool)) -> ASysR<Box<dyn FsInode>> {
        todo!()
    }
    fn unlink_child<'a>(&'a self, name: &'a str, release: bool) -> ASysR<()> {
        todo!()
    }
    fn rmdir_child<'a>(&'a self, name: &'a str) -> ASysR<()> {
        todo!()
    }
    fn bytes(&self) -> SysRet {
        todo!()
    }
    fn reset_data(&self) -> ASysR<()> {
        todo!()
    }
    fn delete(&self) {
        todo!()
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ftl_util::async_tools::ASysRet {
        todo!()
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ftl_util::async_tools::ASysRet {
        todo!()
    }
}
