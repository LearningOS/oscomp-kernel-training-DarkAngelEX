use core::str::FromStr;

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
};
use ftl_util::{
    async_tools::ASysR,
    error::{SysError, SysR},
    sync::{rw_sleep_mutex::RwSleepMutex, Spin},
};

use crate::{fssp::Fs, inode::FsInode};

pub struct TmpFs {}

impl Fs for TmpFs {
    fn root(&self) -> Box<dyn FsInode> {
        Box::new(TmpFsInode::new(true))
    }
}

impl TmpFs {
    pub fn new() -> Box<Self> {
        Box::new(Self {})
    }
}

#[derive(Clone)]
struct TmpFsInode {
    inner: Arc<TmpFsImpl>,
}

impl FsInode for TmpFsInode {
    fn is_dir(&self) -> bool {
        matches!(self.inner.as_ref(), TmpFsImpl::Dir(_))
    }
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move {
            match self.inner.as_ref() {
                TmpFsImpl::File(_) => Err(SysError::ENOTDIR),
                TmpFsImpl::Dir(dir) => dir.search(name).await,
            }
        })
    }
    fn create<'a>(&'a self, name: &'a str, dir: bool) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move {
            match self.inner.as_ref() {
                TmpFsImpl::File(_) => Err(SysError::ENOTDIR),
                TmpFsImpl::Dir(d) => d.create(name, dir).await,
            }
        })
    }
}

impl TmpFsInode {
    fn new(dir: bool) -> Self {
        let inner = if dir {
            TmpFsImpl::Dir(TmpFsDir::new())
        } else {
            TmpFsImpl::File(TmpFsFile::new())
        };
        Self {
            inner: Arc::new(inner),
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
        let d = self
            .subs
            .shared_lock()
            .await
            .get(name)
            .ok_or(SysError::ENOENT)?
            .clone();
        Ok(Box::new(d))
    }
    pub async fn create(&self, name: &str, dir: bool) -> SysR<Box<dyn FsInode>> {
        let mut lk = self.subs.unique_lock().await;
        if lk.get(name).is_some() {
            return Err(SysError::EEXIST);
        }
        let new = TmpFsInode::new(dir);
        lk.try_insert(name.to_string(), new.clone()).ok().unwrap();
        Ok(Box::new(new))
    }
}

struct TmpFsFile {}

impl TmpFsFile {
    pub fn new() -> Self {
        Self {}
    }
}
