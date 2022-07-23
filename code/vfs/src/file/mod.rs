use core::{
    fmt::Debug,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    device::BlockDevice,
    error::{SysError, SysR, SysRet},
    fs::{stat::Stat, DentryType, Seek},
    time::{Instant, TimeSpec},
};

use crate::{
    inode::{FsInode, VfsInode},
    manager::path::Path,
};

pub trait File: Send + Sync + 'static {
    // 这个文件的工作路径
    fn vfs_file(&self) -> SysR<&VfsFile> {
        Err(SysError::ENOENT)
    }
    fn block_device(&self) -> SysR<Arc<dyn BlockDevice>> {
        Err(SysError::ENOTBLK)
    }
    fn into_vfs_file(self: Arc<Self>) -> SysR<Arc<VfsFile>> {
        Err(SysError::ENOENT)
    }
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn can_mmap(&self) -> bool {
        self.can_read_offset() && self.can_write_offset()
    }
    fn can_read_offset(&self) -> bool {
        false
    }
    fn can_write_offset(&self) -> bool {
        false
    }
    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysRet {
        unimplemented!("read_at {}", core::any::type_name::<Self>())
    }
    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysRet {
        unimplemented!("write_at {}", core::any::type_name::<Self>())
    }
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysRet {
        unimplemented!("lseek {}", core::any::type_name::<Self>())
    }
    fn read<'a>(&'a self, buffer: &'a mut [u8]) -> ASysRet;
    fn write<'a>(&'a self, buffer: &'a [u8]) -> ASysRet;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysRet {
        Ok(0)
    }
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move { unimplemented!("stat {}", core::any::type_name::<Self>()) })
    }
    fn utimensat(&self, _times: [TimeSpec; 2], _now: fn() -> Instant) -> ASysRet {
        unimplemented!("utimensat {}", core::any::type_name::<Self>())
    }
}

pub struct VfsFile {
    pub(crate) path: Path,
    pub(crate) inode: Arc<VfsInode>,
    ptr: AtomicUsize, // 当前文件偏移量指针, 只有文件会用到
}

impl Debug for VfsFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "VfsFile name: {}", self.path.dentry.cache.name.name()).unwrap();
        core::fmt::Result::Ok(())
    }
}

impl VfsFile {
    pub(crate) fn from_path(path: Path) -> SysR<Self> {
        let inode = path.inode_s().into_inode()?;
        Ok(Self {
            path,
            inode,
            ptr: AtomicUsize::new(0),
        })
    }
    pub(crate) fn from_path_arc(path: Path) -> SysR<Arc<Self>> {
        Ok(Arc::new(Self::from_path(path)?))
    }
    pub fn is_dir(&self) -> bool {
        self.inode.is_dir()
    }
    fn fsinode(&self) -> &dyn FsInode {
        self.inode.fsinode.as_ref()
    }
    pub fn parent(&self) -> SysR<Option<Arc<Self>>> {
        self.path
            .parent()
            .map(|p| VfsFile::from_path_arc(p))
            .transpose()
    }
    pub async fn read_all(&self) -> SysR<Vec<u8>> {
        let bytes = self.fsinode().bytes()?;
        let mut v = Vec::new();
        v.resize(bytes, 0);
        let n = self.read_at(0, &mut v[..]).await?;
        debug_assert_eq!(v.len(), n);
        Ok(v)
    }
    pub async fn list(&self) -> SysR<Vec<(DentryType, String)>> {
        self.fsinode().list().await
    }
    pub fn path_str(&self) -> Vec<Arc<str>> {
        let mut v = Vec::new();
        let mut cur = Some(self.path.clone());
        while let Some(p) = cur {
            v.push(p.dentry.cache.name());
            cur = p.parent();
        }
        v.reverse();
        v
    }
}

impl File for VfsFile {
    fn vfs_file(&self) -> SysR<&VfsFile> {
        Ok(self)
    }
    fn into_vfs_file(self: Arc<Self>) -> SysR<Arc<VfsFile>> {
        Ok(self)
    }
    fn block_device(&self) -> SysR<Arc<dyn BlockDevice>> {
        self.fsinode().block_device()
    }
    fn readable(&self) -> bool {
        self.inode.readable()
    }
    fn writable(&self) -> bool {
        self.inode.writable()
    }
    fn can_read_offset(&self) -> bool {
        !self.is_dir() && self.readable()
    }
    fn can_write_offset(&self) -> bool {
        !self.is_dir() && self.writable()
    }
    // 以下为文件操作函数, 对目录操作将失败
    fn lseek(&self, offset: isize, whence: Seek) -> SysRet {
        let len = self.fsinode().bytes()?;
        let ptr = &self.ptr;
        let target = match whence {
            Seek::Set => 0isize,
            Seek::Cur => ptr.load(Ordering::Acquire) as isize,
            Seek::End => len as isize,
        }
        .checked_add(offset)
        .ok_or(SysError::EOVERFLOW)?;
        if target < 0 {
            return Err(SysError::EINVAL);
        }
        let target = target as usize;
        ptr.store(target, Ordering::Release);
        Ok(target)
    }
    fn read<'a>(&'a self, buffer: &'a mut [u8]) -> ASysRet {
        let ptr = &self.ptr;
        let offset = ptr.load(Ordering::Acquire);
        self.fsinode().read_at(buffer, (offset, Some(ptr)))
    }
    fn write<'a>(&'a self, buffer: &'a [u8]) -> ASysRet {
        let ptr = &self.ptr;
        let offset = ptr.load(Ordering::Acquire);
        self.fsinode().write_at(buffer, (offset, Some(ptr)))
    }
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysRet {
        self.fsinode().read_at(buf, (offset, None))
    }
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysRet {
        self.fsinode().write_at(buf, (offset, None))
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        self.fsinode().stat(stat)
    }
    fn utimensat(&self, times: [TimeSpec; 2], now: fn() -> Instant) -> ASysRet {
        self.fsinode().utimensat(times, now)
    }
}
