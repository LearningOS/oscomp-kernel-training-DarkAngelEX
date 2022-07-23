use core::{fmt::Debug, sync::atomic::Ordering};

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    device::BlockDevice,
    error::{SysError, SysR, SysRet},
    fs::{stat::Stat, Seek},
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
    fn into_block_device(self: Arc<Self>) -> SysR<Arc<dyn BlockDevice>> {
        Err(SysError::ENOTBLK)
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
        Ok(Self { path, inode })
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
}

impl File for VfsFile {
    fn vfs_file(&self) -> SysR<&VfsFile> {
        Ok(self)
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
        let ptr = self.inode.ptr();
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
        let ptr = self.inode.ptr();
        let offset = ptr.load(Ordering::Acquire);
        self.fsinode().read_at(buffer, (offset, Some(ptr)))
    }
    fn write<'a>(&'a self, buffer: &'a [u8]) -> ASysRet {
        let ptr = self.inode.ptr();
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
}
