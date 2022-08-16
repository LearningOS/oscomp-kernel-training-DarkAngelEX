use core::sync::atomic::AtomicUsize;

use alloc::{boxed::Box, string::String, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysRet},
    fs::{
        stat::{Stat, S_IFCHR},
        DentryType, Seek,
    },
};
use vfs::{File, FsInode};

use crate::{
    config::PAGE_SIZE,
    fs::stdio::{Stdin, Stdout},
};

pub struct TtyInode;

impl FsInode for TtyInode {
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
        (0, 100001)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            *stat = Stat::zeroed();
            stat.st_blksize = PAGE_SIZE as u32;
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
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Stdin.read(buf)
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Stdout.write(buf)
    }
}

impl File for TtyInode {
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        true
    }
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysRet {
        Err(SysError::ESPIPE)
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> ASysRet {
        Stdin.read(write_only)
    }
    fn write<'a>(&'a self, read_only: &'a [u8]) -> ASysRet {
        Stdout.write(read_only)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            *stat = Stat::zeroed();
            stat.st_blksize = PAGE_SIZE as u32;
            stat.st_mode = S_IFCHR | 0o666;
            Ok(())
        })
    }
}
