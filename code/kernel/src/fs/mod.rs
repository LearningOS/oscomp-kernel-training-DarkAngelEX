use core::sync::atomic::{AtomicUsize, Ordering};

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use fat32::{vfs_interface::Fat32Type, BlockDevice};
use ftl_util::{
    async_tools::{ASysR, ASysRet, Async},
    error::{SysError, SysR, SysRet},
    fs::{path, stat::Stat, DentryType, Mode, OpenFlags},
    time::Instant,
};
use vfs::{select::PL, DevAlloc, File, FsInode, VfsClock, VfsFile, VfsManager, VfsSpawner};

use crate::{
    config::FS_CACHE_MAX_SIZE,
    drivers, executor,
    fs::{
        dev::{null::NullInode, tty::TtyInode, zero::ZeroInode},
        proc::ProcType,
    },
    memory::user_ptr::UserInOutPtr,
    timer,
    user::AutoSie,
};

pub mod dev;
pub mod pipe;
pub mod proc;
pub mod stdio;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Iovec {
    pub iov_base: UserInOutPtr<u8>,
    pub iov_len: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Pollfd {
    pub fd: u32,
    pub events: PL,
    pub revents: PL,
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
struct OsDevAllocator;
impl DevAlloc for OsDevAllocator {
    fn box_clone(&self) -> Box<dyn DevAlloc> {
        Box::new(Self)
    }
    fn alloc(&self) -> usize {
        static DEV_ALLOCATOR: AtomicUsize = AtomicUsize::new(0);
        DEV_ALLOCATOR.fetch_add(1, Ordering::Relaxed)
    }
}

pub async fn init() {
    stack_trace!();
    const XF: SysR<Arc<VfsFile>> = Err(SysError::ENOENT);
    let _sie = AutoSie::new();
    let mut vfs = VfsManager::new(FS_CACHE_MAX_SIZE);
    vfs.init_clock(Box::new(SysClock));
    vfs.init_spawner(Box::new(SysSpawner));
    vfs.init_devalloc(Box::new(OsDevAllocator));
    vfs.import_fstype(Box::new(ProcType));
    let mut fat32type = Fat32Type::new();
    fat32type.config_list(1000, 1000);
    fat32type.config_cache(1000, 1000_000);
    fat32type.config_node(100);
    vfs.import_fstype(Box::new(fat32type));
    // 挂载几个全局目录, 这些会使用TmpFs常驻内存
    vfs.set_spec_dentry("dev".to_string());
    vfs.set_spec_dentry("etc".to_string());
    vfs.set_spec_dentry("tmp".to_string());
    vfs.set_spec_dentry("var".to_string());
    vfs.set_spec_dentry("usr".to_string());
    vfs.set_spec_dentry("proc".to_string());

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
    // 挂载FAT32!!!
    vfs.mount((XF, "/dev/sda1"), (XF, "/"), "vfat", 0)
        .await
        .unwrap();
    vfs.mount((XF, ""), (XF, "/proc"), "proc", 0).await.unwrap();
    for path in ["/dev/shm", "/var/tmp", "/dev/misc"] {
        vfs.create((XF, path), true, (true, true)).await.unwrap();
    }
    vfs.create((XF, "/dev/misc/rtc"), false, (true, true))
        .await
        .unwrap();
    // 写入目录 /etc/ld-musl-riscv64-sf.path
    let ld = vfs
        .create((XF, "/etc/ld-musl-riscv64-sf.path"), false, (true, true))
        .await
        .unwrap();
    ld.write_at(0, b"/\0").await.unwrap();

    // 测试性能
    #[allow(unused_imports)]
    if false {
        use crate::hart::{sbi, sfence::*};
        use crate::sync::mutex::*;
        use crate::user::*;
        // let file = vfs
        //     .create((XF, "/tmp/lat_test"), false, (true, true))
        //     .await
        //     .unwrap();

        // let mut bt = alloc::collections::BTreeMap::new();
        // bt.try_insert(0, Arc::new(0)).unwrap();
        // bt.try_insert(1, Arc::new(1)).unwrap();
        // bt.try_insert(2, Arc::new(2)).unwrap();
        let start = crate::timer::now();
        let n = 3000;
        for _ in 0..n {
            let _a = NativeAutoSum::new();
            let _a = NativeAutoSum::new();
            // let _a = bt.get(&2).unwrap().clone();
            // sfence_vma_all_no_global();
            // sbi::console_putchar(1);
        }
        let end = crate::timer::now();
        let dur = end - start;
        println!(
            "do {} using {} ms -> {} ns",
            n,
            dur.as_millis(),
            (dur / n).as_nanos()
        );
        panic!();
    }

    unsafe {
        VFS_MANAGER = Some(vfs);
    }
}

pub fn open_file_fast(
    path: (SysR<Arc<VfsFile>>, &str),
    flags: OpenFlags,
    _mode: Mode,
) -> SysR<Arc<VfsFile>> {
    stack_trace!();
    let _sie = AutoSie::new();
    let rw = flags.read_write()?;
    let vfs = vfs_manager();
    if flags.create() {
        return Err(SysError::EAGAIN);
    }
    let file = vfs.open_fast(path)?;
    if rw.1 && !file.writable() {
        return Err(SysError::EACCES);
    }
    Ok(file)
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
    let _sie = AutoSie::new();
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

pub async fn unlinkat(path: (SysR<Arc<VfsFile>>, &str), dir: bool) -> SysR<()> {
    stack_trace!();
    let _sie = AutoSie::new();
    let vfs = vfs_manager();
    if dir {
        vfs.rmdir(path).await
    } else {
        vfs.unlink(path).await
    }
}

/// 显示根目录的东西
pub async fn list_apps() {
    stack_trace!();
    println!("/**** APPS ****");
    let vfs = vfs_manager();
    let _sie = AutoSie::new();
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
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> ASysR<()> {
        todo!()
    }
    fn detach(&self) -> ASysR<()> {
        todo!()
    }
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        todo!()
    }
    fn search<'a>(&'a self, _name: &'a str) -> ASysR<Box<dyn FsInode>> {
        todo!()
    }
    fn create<'a>(
        &'a self,
        _name: &'a str,
        _dir: bool,
        _rw: (bool, bool),
    ) -> ASysR<Box<dyn FsInode>> {
        todo!()
    }
    fn unlink_child<'a>(&'a self, _name: &'a str, _release: bool) -> ASysR<()> {
        todo!()
    }
    fn rmdir_child<'a>(&'a self, _name: &'a str) -> ASysR<()> {
        todo!()
    }
    fn bytes(&self) -> SysRet {
        todo!()
    }
    fn reset_data(&self) -> ASysR<()> {
        todo!()
    }
    fn read_at<'a>(
        &'a self,
        _buf: &'a mut [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        todo!()
    }
    fn write_at<'a>(
        &'a self,
        _buf: &'a [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        todo!()
    }
}
