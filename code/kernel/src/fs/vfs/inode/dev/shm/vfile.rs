use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::{DentryType, File, Seek, VfsInode},
};

use crate::sync::RwSleepMutex;

static mut ROOT_DIR: Option<Arc<ShmInode>> = None;

pub fn init() {
    unsafe { ROOT_DIR = Some(Arc::new(ShmInode::new_dir())) }
}

pub(super) fn root() -> Arc<ShmInode> {
    unsafe { ROOT_DIR.as_ref().unwrap().clone() }
}

pub(super) struct ShmInode {
    inner: RwSleepMutex<ShmInodeInner>,
}

impl ShmInode {
    pub fn writable(&self) -> bool {
        true
    }
    pub fn is_dir(&self) -> bool {
        match unsafe { self.inner.unsafe_get() } {
            ShmInodeInner::File(_) => false,
            ShmInodeInner::Dir(_) => true,
        }
    }
    pub async fn search_any(&self, name: &str) -> SysR<Arc<ShmInode>> {
        match &*self.inner.shared_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => d.get(name).cloned().ok_or(SysError::ENOENT),
        }
    }
    pub async fn create_dir(&self, name: &str, _read_only: bool) -> SysR<()> {
        match &mut *self.inner.unique_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => {
                match d.try_insert(name.to_string(), Arc::new(ShmInode::new_dir())) {
                    Ok(_) => Ok(()),
                    Err(_e) => Err(SysError::EEXIST),
                }
            }
        }
    }
    pub async fn create_file(&self, name: &str, _read_only: bool) -> SysR<()> {
        match &mut *self.inner.unique_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => {
                match d.try_insert(name.to_string(), Arc::new(ShmInode::new_file())) {
                    Ok(_) => Ok(()),
                    Err(_e) => Err(SysError::EEXIST),
                }
            }
        }
    }
    pub async fn delete_file(&self, name: &str) -> SysR<()> {
        match &mut *self.inner.unique_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => {
                let inode = d.get(name).ok_or(SysError::ENOENT)?;
                if inode.is_dir() {
                    return Err(SysError::EISDIR);
                }
                match d.remove(name) {
                    Some(_) => Ok(()),
                    None => Err(SysError::ENOENT),
                }
            }
        }
    }
    pub async fn delete_dir(&self, name: &str) -> SysR<()> {
        match &mut *self.inner.unique_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => {
                let inode = d.get(name).ok_or(SysError::ENOENT)?;
                if !inode.is_dir() {
                    return Err(SysError::ENOTDIR);
                }
                match d.remove(name) {
                    Some(_) => Ok(()),
                    None => Err(SysError::ENOENT),
                }
            }
        }
    }
    pub async fn delete_any(&self, name: &str) -> SysR<()> {
        match &mut *self.inner.unique_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => match d.remove(name) {
                Some(_) => Ok(()),
                None => Err(SysError::ENOENT),
            },
        }
    }
}

enum ShmInodeInner {
    File(Vec<u8>),
    Dir(BTreeMap<String, Arc<ShmInode>>),
}

impl ShmInode {
    pub fn new_dir() -> Self {
        Self {
            inner: RwSleepMutex::new(ShmInodeInner::Dir(BTreeMap::new())),
        }
    }
    pub fn new_file() -> Self {
        Self {
            inner: RwSleepMutex::new(ShmInodeInner::File(Vec::new())),
        }
    }
    pub async fn search_dir(&self, name: &str) -> SysR<Arc<ShmInode>> {
        match &*self.inner.shared_lock().await {
            ShmInodeInner::File(_) => Err(SysError::ENOTDIR),
            ShmInodeInner::Dir(d) => d.get(name).cloned().ok_or(SysError::ENOENT),
        }
    }
}

pub struct ShmVfile {
    pub(super) readable: AtomicBool,
    pub(super) writable: AtomicBool,
    pub(super) ptr: AtomicUsize,
    pub(super) inode: Arc<ShmInode>,
}

impl ShmVfile {}

impl File for ShmVfile {
    // 这个文件的工作路径
    fn to_vfs_inode(&self) -> SysR<&dyn VfsInode> {
        Ok(self)
    }
    fn readable(&self) -> bool {
        self.readable.load(Ordering::Relaxed)
    }
    fn writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }
    fn can_read_offset(&self) -> bool {
        todo!()
    }
    fn can_write_offset(&self) -> bool {
        todo!()
    }
    fn lseek(&self, _offset: isize, _whence: Seek) -> SysRet {
        unimplemented!("lseek unimplement: {}", core::any::type_name::<Self>())
    }
    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysRet {
        unimplemented!()
    }
    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysRet {
        unimplemented!()
    }
    fn read<'a>(&'a self, _write_only: &'a mut [u8]) -> ASysRet {
        todo!()
    }
    fn write<'a>(&'a self, _read_only: &'a [u8]) -> ASysRet {
        todo!()
    }
}

impl VfsInode for ShmVfile {
    fn read_all(&self) -> ASysR<Vec<u8>> {
        todo!()
    }

    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        todo!()
    }

    fn path(&self) -> &[String] {
        todo!()
    }
}
