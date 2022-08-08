use core::sync::atomic::AtomicUsize;

use alloc::{boxed::Box, vec::Vec, string::String};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysRet},
    fs::{stat::Stat, DentryType},
};
use vfs::FsInode;

pub struct ZeroInode;

impl FsInode for ZeroInode {
    // === 共享操作 ===
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
    // === 目录操作 ===
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn search<'a>(&'a self, _name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn create<'a>(&'a self, _name: &'a str, _dir: bool, _rw: (bool, bool)) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn place_inode<'a>(
        &'a self,
        _name: &'a str,
        _inode: Box<dyn FsInode>,
    ) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn unlink_child<'a>(&'a self, _name: &'a str, _release: bool) -> ASysR<()> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn rmdir_child<'a>(&'a self, _name: &'a str) -> ASysR<()> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }

    // === 文件操作 ===

    fn bytes(&self) -> SysRet {
        Ok(0)
    }
    fn reset_data(&self) -> ASysR<()> {
        Box::pin(async move { Ok(()) })
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            buf.fill(0);
            Ok(buf.len())
        })
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move { Ok(buf.len()) })
    }
}
