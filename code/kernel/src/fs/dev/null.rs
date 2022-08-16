use core::sync::atomic::AtomicUsize;

use alloc::{boxed::Box, string::String, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysRet},
    fs::{
        stat::{Stat, S_IFCHR},
        DentryType,
    },
};
use vfs::FsInode;

pub struct NullInode;

impl FsInode for NullInode {
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
    fn dev_ino(&self) -> (usize, usize) {
        (0, 100000)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            stat.st_mode = S_IFCHR | 0o666;
            Ok(())
        })
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
    fn create<'a>(
        &'a self,
        _name: &'a str,
        _dir: bool,
        _rw: (bool, bool),
    ) -> ASysR<Box<dyn FsInode>> {
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
    fn read_at_fast(
        &self,
        _buf: &mut [u8],
        _offset_with_ptr: (usize, Option<&AtomicUsize>),
    ) -> SysRet {
        Ok(0)
    }
    fn write_at_fast(&self, buf: &[u8], _offset_with_ptr: (usize, Option<&AtomicUsize>)) -> SysRet {
        Ok(buf.len())
    }
    fn read_at<'a>(
        &'a self,
        _buf: &'a mut [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move { Ok(0) })
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move { Ok(buf.len()) })
    }
}
