use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{drivers, executor, syscall::SysResult, tools::xasync::Async, user::AutoSie, timer};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use fat32::Fat32Manager;
use ftl_util::{
    async_tools::AsyncFile,
    error::SysError,
    fs::{
        stat::{Stat, S_IFDIR, S_IFREG},
        DentryType, File, OpenFlags, Seek,
    },
    time::TimeSpec,
    utc_time::UtcTime,
};

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

pub async fn open_file(path: &[&str], flags: OpenFlags) -> Result<Arc<Fat32Inode>, SysError> {
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

pub async fn unlink<'a>(path: &[&str], _flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    manager().delete_any(path).await
}

pub async fn create_any(path: &[&str], flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    let _sie = AutoSie::new();
    let (f_r, _f_w) = flags.read_write()?;
    manager().create_any(path, flags.dir(), f_r, false).await?;
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
    fn lseek(&self, offset: isize, whence: Seek) -> SysResult {
        let len = self.inode.file().ok_or(SysError::EISDIR)?.bytes();
        let target = match whence {
            Seek::Set => 0isize,
            Seek::Cur => self.ptr.load(Ordering::Acquire) as isize,
            Seek::End => len as isize,
        }
        .checked_add(offset)
        .ok_or(SysError::EOVERFLOW)?;
        let target = usize::try_from(target).map_err(|_e| SysError::EINVAL)?;
        self.ptr.store(target, Ordering::Release);
        Ok(target)
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
    fn utimensat(&self, times: [TimeSpec; 2]) -> Async<SysResult> {
        Box::pin(async move {
            let [access, modify] = times.try_map(timer::time_sepc_to_utc)?;
            self.inode
                .update_time(access.as_ref(), modify.as_ref())
                .await;
            Ok(0)
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
