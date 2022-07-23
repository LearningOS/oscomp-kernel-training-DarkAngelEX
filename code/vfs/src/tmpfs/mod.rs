use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::stat::Stat,
    sync::{rw_sleep_mutex::RwSleepMutex, Spin},
};

use crate::{
    fssp::{Fs, FsType},
    inode::FsInode,
    VfsFile,
};

pub struct TmpFsInfo;

impl TmpFsInfo {
    pub fn new() -> Box<dyn FsType> {
        Box::new(Self)
    }
}

impl FsType for TmpFsInfo {
    fn name(&self) -> String {
        "tmpfs".to_string()
    }
    fn new_fs(&self) -> Box<dyn Fs> {
        TmpFs::new()
    }
}

struct TmpFs {}

impl Fs for TmpFs {
    fn need_src(&self) -> bool {
        false
    }
    fn init(&mut self, _file: Option<VfsFile>, _flags: usize) -> ASysR<()> {
        Box::pin(async { Ok(()) })
    }
    fn root(&self) -> Box<dyn FsInode> {
        Box::new(TmpFsInode::new(true, (true, true)))
    }
}

impl TmpFs {
    pub fn new() -> Box<Self> {
        Box::new(Self {})
    }
}

struct TmpFsInode {
    readable: AtomicBool,
    writable: AtomicBool,
    inner: Arc<TmpFsImpl>,
}

impl Clone for TmpFsInode {
    fn clone(&self) -> Self {
        Self {
            readable: AtomicBool::new(self.readable.load(Ordering::Relaxed)),
            writable: AtomicBool::new(self.writable.load(Ordering::Relaxed)),
            inner: self.inner.clone(),
        }
    }
}

impl FsInode for TmpFsInode {
    fn readable(&self) -> bool {
        self.readable.load(Ordering::Relaxed)
    }
    fn writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }
    fn is_dir(&self) -> bool {
        matches!(self.inner.as_ref(), TmpFsImpl::Dir(_))
    }
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { self.dir()?.search(name).await })
    }
    fn create<'a>(&'a self, name: &'a str, dir: bool, rw: (bool, bool)) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move { self.dir()?.create(name, dir, rw).await })
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
        Box::pin(async move { self.file()?.reset_data().await })
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        (offset, ptr): (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            let n = self.file()?.read_at(offset, buf).await?;
            ptr.map(|p| p.store(offset + n, Ordering::Release));
            Ok(n)
        })
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        (offset, ptr): (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            let n = self.file()?.write_at(offset, buf).await?;
            ptr.map(|p| p.store(offset + n, Ordering::Release));
            Ok(n)
        })
    }
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            match self.inner.as_ref() {
                TmpFsImpl::Dir(_) => Err(SysError::EISDIR),
                TmpFsImpl::File(f) => f.reset_data().await,
            }
        })
    }
}

impl TmpFsInode {
    fn new(dir: bool, (r, w): (bool, bool)) -> Self {
        let inner = if dir {
            TmpFsImpl::Dir(TmpFsDir::new())
        } else {
            TmpFsImpl::File(TmpFsFile::new())
        };
        Self {
            readable: AtomicBool::new(r),
            writable: AtomicBool::new(w),
            inner: Arc::new(inner),
        }
    }
    fn dir(&self) -> SysR<&TmpFsDir> {
        match self.inner.as_ref() {
            TmpFsImpl::File(_) => Err(SysError::ENOTDIR),
            TmpFsImpl::Dir(dir) => Ok(dir),
        }
    }
    fn file(&self) -> SysR<&TmpFsFile> {
        match self.inner.as_ref() {
            TmpFsImpl::Dir(_) => Err(SysError::EISDIR),
            TmpFsImpl::File(file) => Ok(file),
        }
    }
}

enum TmpFsImpl {
    Dir(TmpFsDir),
    File(TmpFsFile),
}

struct TmpFsDir {
    subs: RwSleepMutex<BTreeMap<String, TmpFsInode>, Spin>,
}

impl TmpFsDir {
    pub fn new() -> Self {
        Self {
            subs: RwSleepMutex::new(BTreeMap::new()),
        }
    }
    pub async fn search(&self, name: &str) -> SysR<Box<dyn FsInode>> {
        let lk = self.subs.shared_lock().await;
        let d = lk.get(name).ok_or(SysError::ENOENT)?;
        Ok(Box::new(d.clone()))
    }
    pub async fn create(&self, name: &str, dir: bool, rw: (bool, bool)) -> SysR<Box<dyn FsInode>> {
        let mut lk = self.subs.unique_lock().await;
        if lk.get(name).is_some() {
            return Err(SysError::EEXIST);
        }
        let new = TmpFsInode::new(dir, rw);
        lk.try_insert(name.to_string(), new.clone()).ok().unwrap();
        Ok(Box::new(new))
    }
    pub async fn unlink_child<'a>(&'a self, name: &'a str, _release: bool) -> SysR<()> {
        let mut lk = self.subs.unique_lock().await;
        let sub = lk.get(name).ok_or(SysError::ENOENT)?;
        if sub.is_dir() {
            return Err(SysError::EISDIR);
        }
        let _f = lk.remove(name).unwrap();
        Ok(())
    }
    pub async fn rmdir_child<'a>(&'a self, name: &'a str) -> SysR<()> {
        let mut lk = self.subs.unique_lock().await;
        let sub = lk.get(name).ok_or(SysError::ENOENT)?.dir()?;
        if unsafe { !sub.subs.unsafe_get().is_empty() } {
            return Err(SysError::ENOTEMPTY);
        }
        let _sub = lk.remove(name).unwrap();
        Ok(())
    }
}

struct TmpFsFile {
    subs: RwSleepMutex<Vec<u8>, Spin>,
}

impl TmpFsFile {
    pub fn new() -> Self {
        Self {
            subs: RwSleepMutex::new(Vec::new()),
        }
    }
    pub fn bytes(&self) -> SysRet {
        unsafe {
            let n = self.subs.unsafe_get().len();
            Ok(n)
        }
    }
    pub async fn reset_data(&self) -> SysR<()> {
        let mut lk = self.subs.unique_lock().await;
        lk.clear();
        lk.shrink_to_fit();
        Ok(())
    }
    pub async fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> SysRet {
        let lk = self.subs.shared_lock().await;
        let end = lk.len().min(offset + buf.len());
        let n = end - offset;
        buf[..n].copy_from_slice(&lk[offset..end]);
        Ok(n)
    }
    pub async fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> SysRet {
        let expand = offset + buf.len() > self.bytes()?;
        if !expand {
            let lk = self.subs.shared_lock().await;
            let end = offset + buf.len();
            if end <= lk.len() {
                unsafe {
                    (*(&lk[offset..end] as *const _ as *mut [u8])).copy_from_slice(buf);
                }
                return Ok(buf.len());
            }
        }
        let mut lk = self.subs.unique_lock().await;
        let end = offset + buf.len();
        if end > lk.len() {
            lk.resize(end, 0);
        }
        lk[offset..end].copy_from_slice(buf);
        Ok(buf.len())
    }
}
