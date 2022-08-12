use core::sync::atomic::AtomicUsize;

use alloc::{boxed::Box, string::String, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysRet},
    fs::{stat::Stat, DentryType},
};
use vfs::FsInode;

pub struct MountInode;

impl MountInode {
    pub fn new_dyn() -> Box<dyn FsInode> {
        Box::new(Self)
    }
}

impl FsInode for MountInode {
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        false
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
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
    fn search<'a>(&'a self, _name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { Err(SysError::ENOENT) })
    }
    fn create<'a>(
        &'a self,
        _name: &'a str,
        _dir: bool,
        _rw: (bool, bool),
    ) -> ASysR<Box<dyn FsInode>> {
        todo!()
    }
    fn unlink_child<'a>(&'a self, _name: &'a str, _release: bool) -> ASysR<()> {
        todo!()
    }
    fn rmdir_child<'a>(&'a self, _name: &'a str) -> ASysR<()> {
        todo!()
    }
    fn bytes(&self) -> SysRet {
        todo!()
    }
    fn reset_data(&self) -> ASysR<()> {
        todo!()
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
        _buf: &'a [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        todo!()
    }
}
