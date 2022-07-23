mod tdir;
mod tfile;

use core::sync::atomic::AtomicUsize;

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::stat::Stat,
};

use crate::{
    fssp::{Fs, FsType},
    inode::FsInode,
    manager::{VfsClock, VfsSpawner},
    VfsFile,
};

use self::{tdir::TmpFsDir, tfile::TmpFsFile};

pub struct TmpFsType;

impl TmpFsType {
    pub fn new() -> Box<dyn FsType> {
        Box::new(Self)
    }
}

impl FsType for TmpFsType {
    fn name(&self) -> String {
        "tmpfs".to_string()
    }
    fn new_fs(&self) -> Box<dyn Fs> {
        TmpFs::new()
    }
}

pub(crate) struct TmpFs {}

impl Fs for TmpFs {
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
    ) -> ASysR<()> {
        Box::pin(async { Ok(()) })
    }
    fn set_spawner(&mut self, _spawner: Box<dyn VfsSpawner>) -> ASysR<()> {
        panic!()
    }
    fn root(&self) -> Box<dyn FsInode> {
        Box::new(TmpFsInode::new(true, (true, true)))
    }
}

impl TmpFs {
    pub fn new() -> Box<Self> {
        Box::new(Self {})
    }
    pub fn new_dir() -> Box<dyn FsInode> {
        Box::new(TmpFsInode::new(true, (true, true)))
    }
}

#[derive(Clone)]
struct TmpFsInode(Arc<TmpFsImpl>);

enum TmpFsImpl {
    Dir(TmpFsDir),
    File(Box<dyn FsInode>),
}

impl TmpFsInode {
    fn new(dir: bool, rw: (bool, bool)) -> Self {
        if dir {
            Self::from_impl(TmpFsImpl::Dir(TmpFsDir::new(rw)))
        } else {
            Self::from_impl(TmpFsImpl::File(Box::new(TmpFsFile::new(rw))))
        }
    }
    fn new_inode(inode: Box<dyn FsInode>) -> Self {
        Self::from_impl(TmpFsImpl::File(inode))
    }
    fn from_impl(tfi: TmpFsImpl) -> Self {
        Self(Arc::new(tfi))
    }
    fn dir(&self) -> SysR<&TmpFsDir> {
        match self.0.as_ref() {
            TmpFsImpl::Dir(d) => Ok(d),
            TmpFsImpl::File(_) => Err(SysError::ENOTDIR),
        }
    }
    fn file(&self) -> SysR<&dyn FsInode> {
        match self.0.as_ref() {
            TmpFsImpl::Dir(_) => Err(SysError::EISDIR),
            TmpFsImpl::File(f) => Ok(f.as_ref()),
        }
    }
}

impl FsInode for TmpFsInode {
    fn readable(&self) -> bool {
        match self.0.as_ref() {
            TmpFsImpl::Dir(d) => d.readable(),
            TmpFsImpl::File(f) => f.readable(),
        }
    }
    fn writable(&self) -> bool {
        match self.0.as_ref() {
            TmpFsImpl::Dir(d) => d.writable(),
            TmpFsImpl::File(f) => f.writable(),
        }
    }
    fn is_dir(&self) -> bool {
        match self.0.as_ref() {
            TmpFsImpl::Dir(_) => true,
            TmpFsImpl::File(_) => false,
        }
    }
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { self.dir()?.search(name).await })
    }
    fn create<'a>(&'a self, name: &'a str, dir: bool, rw: (bool, bool)) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { self.dir()?.create(name, dir, rw).await })
    }
    fn place_inode<'a>(
        &'a self,
        name: &'a str,
        inode: Box<dyn FsInode>,
    ) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { self.dir()?.place_inode(name, inode).await })
    }
    fn unlink_child<'a>(&'a self, name: &'a str, release: bool) -> ASysR<()> {
        Box::pin(async move { self.dir()?.unlink_child(name, release).await })
    }
    fn rmdir_child<'a>(&'a self, name: &'a str) -> ASysR<()> {
        Box::pin(async move { self.dir()?.rmdir_child(name).await })
    }
    fn bytes(&self) -> SysRet {
        self.file()?.bytes()
    }
    fn reset_data(&self) -> ASysR<()> {
        self.file().unwrap().reset_data()
    }
    fn delete(&self) {
        self.file().unwrap().delete()
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        self.file().unwrap().read_at(buf, offset_with_ptr)
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        self.file().unwrap().write_at(buf, offset_with_ptr)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        match self.0.as_ref() {
            TmpFsImpl::Dir(d) => Box::pin(async move { d.stat(stat).await }),
            TmpFsImpl::File(f) => f.stat(stat),
        }
    }
}
