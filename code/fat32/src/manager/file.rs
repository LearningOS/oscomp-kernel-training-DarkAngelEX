use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::{
        stat::{Stat, S_IFDIR, S_IFREG},
        DentryType, Seek,
    },
    time::{Instant, TimeSpec},
};
use vfs::File;

use crate::{AnyInode, Fat32Manager};

pub struct Fat32Inode {
    readable: AtomicBool,
    writable: AtomicBool,
    ptr: AtomicUsize,
    inode: AnyInode,
    path: Vec<String>,
    manager: Arc<Fat32Manager>,
    ino: usize,
}

impl Fat32Inode {
    pub fn path(&self) -> &[String] {
        &self.path
    }
    pub async fn read_all(&self) -> SysR<Vec<u8>> {
        stack_trace!();
        let buffer_size = 4096;
        let mut buffer = unsafe { Box::try_new_uninit_slice(buffer_size)?.assume_init() };
        let mut v: Vec<u8> = Vec::new();
        let file = self.inode.file()?;
        let mut offset = 0;
        loop {
            let len = file.read_at(&self.manager, offset, &mut *buffer).await?;
            if len == 0 {
                break;
            }
            offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v.shrink_to_fit();
        Ok(v)
    }
    pub async fn list(&self) -> SysR<Vec<(DentryType, String)>> {
        self.inode.dir()?.list(&self.manager).await
    }
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
    fn lseek(&self, offset: isize, whence: Seek) -> SysRet {
        let len = self.inode.file()?.bytes();
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
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysRet {
        Box::pin(async move {
            let inode = self.inode.file()?;
            let n = inode.read_at(&self.manager, offset, buf).await?;
            Ok(n)
        })
    }
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysRet {
        Box::pin(async move {
            let inode = self.inode.file()?;
            let n = inode.write_at(&self.manager, offset, buf).await?;
            Ok(n)
        })
    }
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> ASysRet {
        Box::pin(async move {
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = self.inode.file()?;
            let n = inode.read_at(&self.manager, offset, buf).await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
    fn write<'a>(&'a self, buf: &'a [u8]) -> ASysRet {
        Box::pin(async move {
            // println!("write: {}", buf.len());
            let offset = self.ptr.load(Ordering::Acquire);
            let inode = self.inode.file()?;
            let n = inode.write_at(&self.manager, offset, buf).await?;
            self.ptr.store(offset + n, Ordering::Release);
            Ok(n)
        })
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            let bpb = self.manager.bpb();
            let short = self.inode.short_name();
            let size = short.file_bytes();
            let blk_size = bpb.cluster_bytes as u32;
            let blk_n = self.inode.blk_num(&self.manager).await? as u64;
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
    fn utimensat(&self, times: [TimeSpec; 2], now: fn() -> Instant) -> ASysRet {
        Box::pin(async move {
            let [access, modify] = times
                .try_map(|v| v.user_map(now))?
                .map(|v| v.map(|v| v.as_instant()));
            self.inode.update_time(access, modify).await;
            Ok(0)
        })
    }
}
