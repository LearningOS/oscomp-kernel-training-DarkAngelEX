use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{
    drivers, executor,
    fs::{stat::Stat, AsyncFile, File, OpenFlags},
    tools::{path, xasync::Async},
    user::AutoSie,
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use fat32::Fat32Manager;
use ftl_util::{error::SysError, fs::DentryType, utc_time::UtcTime};

pub use fat32::AnyInode;

use super::VfsInode;

pub struct Fat32Inode {
    readable: AtomicBool,
    writable: AtomicBool,
    ptr: AtomicUsize,
    inode: AnyInode,
    path: Vec<String>,
}

impl Fat32Inode {
    pub fn path(&self) -> &[String] {
        &self.path
    }
    pub async fn read_all(&self) -> Result<Vec<u8>, SysError> {
        stack_trace!();
        let buffer_size = 4096;
        let mut buffer = unsafe { Box::try_new_uninit_slice(buffer_size)?.assume_init() };
        let mut v: Vec<u8> = Vec::new();
        let file = self.inode.file().ok_or(SysError::EISDIR)?;
        let mut offset = 0;
        loop {
            let len = file.read_at(manager(), offset, &mut *buffer).await?;
            if len == 0 {
                break;
            }
            offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v.shrink_to_fit();
        Ok(v)
    }
    pub async fn list(&self) -> Result<Vec<(DentryType, String)>, SysError> {
        let dir = self.inode.dir().ok_or(SysError::ENOTDIR)?;
        dir.list(manager()).await
    }
}

static mut MANAGER: Option<Fat32Manager> = None;

fn manager() -> &'static Fat32Manager {
    unsafe { MANAGER.as_ref().unwrap() }
}

pub async fn init() {
    stack_trace!();
    let _sie = AutoSie::new();
    unsafe {
        MANAGER = Some(Fat32Manager::new(100, 200, 100, 200, 100));
        let manager = MANAGER.as_mut().unwrap();
        manager
            .init(drivers::device().clone(), Box::new(|| UtcTime::base()))
            .await;
        manager
            .spawn_sync_task((8, 8), |f| executor::kernel_spawn(f))
            .await;
    }
}

pub async fn list_apps() {
    stack_trace!();
    let _sie = AutoSie::new();
    println!("/**** APPS ****");
    for (dt, name) in manager().root_dir().list(manager()).await.unwrap() {
        println!("{} {:?}", name, dt);
    }
    println!("**************/");
}

pub async fn open_file<'a>(path: &[&str], flags: OpenFlags) -> Result<Arc<Fat32Inode>, SysError> {
    stack_trace!();
    let _sie = AutoSie::new();
    let (f_r, f_w) = flags.read_write()?;
    // println!("open_file {:?} flags: {:#x}, create: {}", stack, flags, flags.create());
    if flags.create() {
        match manager().create_any(&path, flags.dir(), !f_w, false).await {
            Ok(_) => (),
            Err(SysError::EEXIST) => {
                if flags.dir() {
                    return Err(SysError::EISDIR);
                }
                manager().delete_file(&path).await?;
                manager().create_any(&path, false, !f_w, false).await?;
            }
            Err(e) => return Err(e),
        }
    }
    let inode = manager().search_any(&path).await?;
    if f_w && !inode.attr().writable() {
        return Err(SysError::EACCES);
    }
    let path = path.into_iter().map(|s| s.to_string()).collect();
    Ok(Arc::new(Fat32Inode {
        readable: AtomicBool::new(f_r),
        writable: AtomicBool::new(f_w),
        ptr: AtomicUsize::new(0),
        inode,
        path,
    }))
}

pub async fn unlink<'a>(
    base: Option<Result<impl Iterator<Item = &'a str>, SysError>>,
    path: &'a str,
    _flags: OpenFlags,
) -> Result<(), SysError> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => path::walk_iter_path(base.unwrap()?, &mut stack),
    }
    path::walk_path(path, &mut stack);
    manager().delete_any(&stack).await
}

pub async fn create_any<'a>(
    base: Option<Result<impl Iterator<Item = &'a str>, SysError>>,
    path: &'a str,
    flags: OpenFlags,
) -> Result<(), SysError> {
    stack_trace!();
    let _sie = AutoSie::new();
    let (f_r, _f_w) = flags.read_write()?;
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => path::walk_iter_path(base.unwrap()?, &mut stack),
    }
    path::walk_path(path, &mut stack);
    manager()
        .create_any(&stack, flags.dir(), f_r, false)
        .await?;
    Ok(())
}

impl File for Fat32Inode {
    fn to_vfs_inode(&self) -> Result<&dyn VfsInode, SysError> {
        Ok(self)
    }
    fn readable(&self) -> bool {
        self.readable.load(Ordering::Relaxed)
    }
    fn writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }
    fn can_read_offset(&self) -> bool {
        true
    }
    fn can_write_offset(&self) -> bool {
        true
    }
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.read_at(manager(), offset, buf).await?;
            Ok(n)
        })
    }
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, buf).await?;
            Ok(n)
        })
    }
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> AsyncFile {
        Box::pin(async move {
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.read_at(manager(), offset, buf).await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
    fn write<'a>(&'a self, buf: &'a [u8]) -> AsyncFile {
        Box::pin(async move {
            // println!("write: {}", buf.len());
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, buf).await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> Async<'a, Result<(), SysError>> {
        Box::pin(async move {
            let bpb = manager().bpb();
            let short = self.inode.short_name();
            let size = short.file_bytes();
            let blk_size = bpb.cluster_bytes as u32;
            let blk_n = self.inode.blk_num(manager()).await? as u64;
            let access_time = short.access_time();
            let modify_time = short.modify_time();
            stat.st_dev = 0;
            stat.st_ino = 0;
            stat.st_mode = 0o777;
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
}

impl VfsInode for Fat32Inode {
    fn read_all(&self) -> Async<Result<Vec<u8>, SysError>> {
        Box::pin(self.read_all())
    }

    fn list(&self) -> Async<Result<Vec<(DentryType, String)>, SysError>> {
        Box::pin(self.list())
    }

    fn path(&self) -> &[String] {
        self.path()
    }
}
