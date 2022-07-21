use core::time::Duration;

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::{stat::Stat, Seek},
    time::TimeSpec,
};

use crate::{inode::VfsInode, manager::path::Path};

pub trait File: Send + Sync + 'static {
    // 这个文件的工作路径
    fn vfs_file(&self) -> SysR<&VfsFile> {
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
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysRet {
        unimplemented!("lseek {}", core::any::type_name::<Self>())
    }
    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysRet {
        unimplemented!("read_at {}", core::any::type_name::<Self>())
    }
    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysRet {
        unimplemented!("write_at {}", core::any::type_name::<Self>())
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> ASysRet;
    fn write<'a>(&'a self, read_only: &'a [u8]) -> ASysRet;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysRet {
        Ok(0)
    }
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move { Err(SysError::EACCES) })
    }
    fn utimensat(&self, _times: [TimeSpec; 2], _now: fn() -> Duration) -> ASysRet {
        unimplemented!("utimensat {}", core::any::type_name::<Self>())
    }
}

pub struct VfsFile {
    pub(crate) path: Path,
    pub(crate) inode: Arc<VfsInode>,
}

impl VfsFile {
    pub(crate) fn from_path(path: Path) -> SysR<Self> {
        let inode = path.get_inode().ok_or(SysError::ENOENT)?;
        Ok(Self { path, inode })
    }
}

impl File for VfsFile {
    fn vfs_file(&self) -> SysR<&VfsFile> {
        Ok(self)
    }
    fn readable(&self) -> bool {
        todo!()
    }
    fn writable(&self) -> bool {
        todo!()
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> ASysRet {
        todo!()
    }
    fn write<'a>(&'a self, read_only: &'a [u8]) -> ASysRet {
        todo!()
    }
}
