mod meminfo;
mod mounts;

use core::sync::atomic::AtomicUsize;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysRet},
    fs::{stat::Stat, DentryType},
};
use vfs::{Fs, FsInode, FsType, VfsClock, VfsFile, VfsSpawner};

use self::{meminfo::MeminfoInode, mounts::MountInode};

pub struct ProcType;

impl FsType for ProcType {
    fn name(&self) -> String {
        "proc".to_string()
    }
    fn new_fs(&self, _dev: usize) -> Box<dyn Fs> {
        Box::new(ProcFs)
    }
}

struct ProcFs;

impl Fs for ProcFs {
    fn need_src(&self) -> bool {
        false
    }
    fn need_spawner(&self) -> bool {
        false
    }
    fn init(
        &mut self,
        _file: Option<Arc<VfsFile>>,
        _flags: usize,
        _clock: Box<dyn VfsClock>,
    ) -> fat32::ASysR<()> {
        Box::pin(async move { Ok(()) })
    }

    fn set_spawner(&mut self, _spawner: Box<dyn VfsSpawner>) -> ASysR<()> {
        unreachable!()
    }

    fn root(&self) -> Box<dyn FsInode> {
        Box::new(ProcRoot)
    }
}

struct ProcRoot;

impl FsInode for ProcRoot {
    fn readable(&self) -> bool {
        todo!()
    }
    fn writable(&self) -> bool {
        todo!()
    }
    fn is_dir(&self) -> bool {
        todo!()
    }
    fn dev_ino(&self) -> (usize, usize) {
        todo!()
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
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move {
            match name {
                "mounts" => Ok(MountInode::new_dyn()),
                "meminfo" => Ok(MeminfoInode::new_dyn()),
                _ => Err(SysError::ENOENT),
            }
        })
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
        todo!()
    }

    fn write_at<'a>(
        &'a self,
        _buf: &'a [u8],
        _offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        todo!()
    }
}
