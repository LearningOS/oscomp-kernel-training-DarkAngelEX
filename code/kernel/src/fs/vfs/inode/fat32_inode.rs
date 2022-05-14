use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use fat32::Fat32Manager;
use ftl_util::{error::SysError, utc_time::UtcTime};

use crate::{
    drivers, executor,
    fs::{stat::Stat, AsyncFile, File, OpenFlags},
    tools::xasync::Async,
    user::{AutoSie, UserData, UserDataMut},
};

pub use fat32::AnyInode;

pub struct Fat32Inode {
    readable: AtomicBool,
    writable: AtomicBool,
    ptr: AtomicUsize,
    inode: AnyInode,
}

impl Fat32Inode {
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
    for app in manager().root_dir().list(manager()).await.unwrap() {
        println!("{}", app);
    }
    println!("**************/");
}

fn walk_path<'a>(src: &'a str, dst: &mut Vec<&'a str>) {
    for s in src.split(['/', '\\']).map(|s| s.trim()) {
        match s {
            "" | "." => continue,
            ".." => {
                dst.pop();
            }
            s => {
                dst.push(s);
            }
        }
    }
}

pub async fn open_file(
    base: &str,
    path: &str,
    flags: OpenFlags,
) -> Result<Arc<Fat32Inode>, SysError> {
    stack_trace!();
    let _sie = AutoSie::new();
    let (f_r, f_w) = flags.read_write()?;
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => walk_path(base, &mut stack),
    }
    walk_path(path, &mut stack);
    // println!("open_file {:?} flags: {:#x}, create: {}", stack, flags, flags.create());
    if flags.create() {
        match manager().create_any(&stack, flags.dir(), !f_w, false).await {
            Ok(_) => (),
            Err(SysError::EEXIST) => {
                if flags.dir() {
                    return Err(SysError::EISDIR);
                }
                manager().delete_file(&stack).await?;
                manager().create_any(&stack, false, !f_w, false).await?;
            }
            Err(e) => return Err(e),
        }
    }
    let inode = manager().search_any(&stack).await?;
    if !inode.attr().writable() && f_w {
        return Err(SysError::EACCES);
    }
    Ok(Arc::new(Fat32Inode {
        readable: AtomicBool::new(f_r),
        writable: AtomicBool::new(f_w),
        ptr: AtomicUsize::new(0),
        inode,
    }))
}

pub async fn unlink(base: &str, path: &str, _flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => walk_path(base, &mut stack),
    }
    walk_path(path, &mut stack);
    manager().delete_any(&stack).await
}

pub async fn create_any(base: &str, path: &str, flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    let _sie = AutoSie::new();
    let (f_r, f_w) = flags.read_write()?;
    let mut stack = Vec::new();
    match path.as_bytes().first() {
        Some(b'/') => (),
        _ => walk_path(base, &mut stack),
    }
    walk_path(path, &mut stack);
    manager()
        .create_any(&stack, flags.dir(), f_r, false)
        .await?;
    Ok(())
}

impl File for Fat32Inode {
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
    fn read_at(&self, offset: usize, buf: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode
                .read_at(manager(), offset, &mut *buf.access_mut())
                .await?;
            Ok(n)
        })
    }
    fn write_at(&self, offset: usize, buf: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, &*buf.access()).await?;
            Ok(n)
        })
    }
    fn read_at_kernel<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.read_at(manager(), offset, buf).await?;
            Ok(n)
        })
    }
    fn write_at_kernel<'a>(&'a self, offset: usize, buf: &'a [u8]) -> AsyncFile {
        Box::pin(async move {
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, buf).await?;
            Ok(n)
        })
    }
    fn read(&self, buf: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode
                .read_at(manager(), offset, &mut *buf.access_mut())
                .await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
    fn write(&self, buf: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            // println!("write: {}", buf.len());
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = match &self.inode {
                AnyInode::Dir(_) => return Err(SysError::EISDIR),
                AnyInode::File(inode) => inode,
            };
            let n = inode.write_at(manager(), offset, &*buf.access()).await?;
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
