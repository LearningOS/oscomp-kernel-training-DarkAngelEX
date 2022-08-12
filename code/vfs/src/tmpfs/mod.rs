mod tdir;
mod tfile;

use core::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    device::BlockDevice,
    error::{SysError, SysR, SysRet},
    fs::{stat::Stat, DentryType},
    time::{Instant, TimeSpec},
};

use crate::{
    fssp::{Fs, FsType},
    inode::FsInode,
    manager::{VfsClock, VfsSpawner},
    select::PL,
    VfsFile,
};

use self::{tdir::TmpFsDir, tfile::TmpFsFile};

pub struct TmpFsType;

impl TmpFsType {
    pub fn box_new() -> Box<dyn FsType> {
        Box::new(Self)
    }
}

impl FsType for TmpFsType {
    fn name(&self) -> String {
        "tmpfs".to_string()
    }
    fn new_fs(&self, dev: usize) -> Box<dyn Fs> {
        TmpFs::new(dev)
    }
}

pub(crate) struct TmpFs {
    dev: usize,
    root: TmpFsInode,
    inoalloc: AtomicUsize,
}

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
        Box::new(self.root.clone())
    }
}

impl TmpFs {
    pub fn new(dev: usize) -> Box<Self> {
        let root = TmpFsInode::new(true, (true, true), 1, NonNull::dangling());
        let fs = Box::new(Self {
            dev,
            root,
            inoalloc: AtomicUsize::new(2),
        });
        unsafe { fs.root.dir().unwrap().set_fs(fs.ptr()) };
        fs
    }
    pub fn alloc_ino(&self) -> usize {
        self.inoalloc.fetch_add(1, Ordering::Relaxed)
    }
    pub fn new_dir(&self) -> Box<dyn FsInode> {
        Box::new(TmpFsInode::new(
            true,
            (true, true),
            self.alloc_ino(),
            self.ptr(),
        ))
    }
    pub fn ptr(&self) -> NonNull<Self> {
        NonNull::new(self as *const _ as *mut _).unwrap()
    }
}

#[derive(Clone)]
struct TmpFsInode(Arc<TmpFsImpl>);

enum TmpFsImpl {
    Dir(TmpFsDir),
    File(Box<dyn FsInode>),
}

impl TmpFsInode {
    fn new(dir: bool, rw: (bool, bool), ino: usize, fs: NonNull<TmpFs>) -> Self {
        if dir {
            Self::from_impl(TmpFsImpl::Dir(TmpFsDir::new(rw, ino, fs)))
        } else {
            Self::from_impl(TmpFsImpl::File(Box::new(TmpFsFile::new(rw, ino, fs))))
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
    fn block_device(&self) -> SysR<Arc<dyn BlockDevice>> {
        self.file()?.block_device()
    }
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
    fn ppoll(&self) -> PL {
        match self.0.as_ref() {
            TmpFsImpl::File(f) => f.ppoll(),
            TmpFsImpl::Dir(_) => unimplemented!(),
        }
    }
    /// Tmpfs不需要detach操作
    fn detach(&self) -> ASysR<()> {
        Box::pin(async move { Ok(()) })
    }
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        Box::pin(async move { self.dir()?.list().await })
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
    fn read_at_fast(
        &self,
        buf: &mut [u8],
        offset_with_ptr: (usize, Option<&AtomicUsize>),
    ) -> SysRet {
        self.file()?.read_at_fast(buf, offset_with_ptr)
    }
    fn write_at_fast(&self, buf: &[u8], offset_with_ptr: (usize, Option<&AtomicUsize>)) -> SysRet {
        self.file()?.write_at_fast(buf, offset_with_ptr)
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
    fn stat_fast(&self, stat: &mut Stat) -> SysR<()> {
        match self.0.as_ref() {
            TmpFsImpl::File(f) => f.stat_fast(stat),
            TmpFsImpl::Dir(d) => d.stat_fast(stat),
        }
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        match self.0.as_ref() {
            TmpFsImpl::File(f) => f.stat(stat),
            TmpFsImpl::Dir(d) => Box::pin(async move { d.stat(stat).await }),
        }
    }
    fn utimensat(&self, times: [TimeSpec; 2], now: fn() -> Instant) -> ASysR<()> {
        match self.0.as_ref() {
            TmpFsImpl::Dir(_d) => todo!(),
            TmpFsImpl::File(f) => f.utimensat(times, now),
        }
    }
}
