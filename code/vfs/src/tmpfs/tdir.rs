use core::sync::atomic::{AtomicBool, Ordering};

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
use ftl_util::{
    error::{SysError, SysR},
    fs::{stat::Stat, DentryType},
    sync::{rw_sleep_mutex::RwSleepMutex, Spin},
};

use crate::FsInode;

use super::TmpFsInode;

pub struct TmpFsDir {
    readable: AtomicBool,
    writable: AtomicBool,
    subs: RwSleepMutex<BTreeMap<String, TmpFsInode>, Spin>,
}

impl TmpFsDir {
    pub fn new((r, w): (bool, bool)) -> Self {
        Self {
            readable: AtomicBool::new(r),
            writable: AtomicBool::new(w),
            subs: RwSleepMutex::new(BTreeMap::new()),
        }
    }
    pub async fn search(&self, name: &str) -> SysR<Box<dyn FsInode>> {
        let lk = self.subs.shared_lock().await;
        let d = lk.get(name).ok_or(SysError::ENOENT)?.clone();
        Ok(Box::new(d))
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
    pub async fn place_inode<'a>(
        &'a self,
        name: &'a str,
        inode: Box<dyn FsInode>,
    ) -> SysR<Box<dyn FsInode>> {
        debug_assert!(!inode.is_dir());
        let mut lk = self.subs.unique_lock().await;
        if lk.get(name).is_some() {
            return Err(SysError::EEXIST);
        }
        let new = TmpFsInode::new_inode(inode);
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
        let child = lk.get(name).ok_or(SysError::ENOENT)?;
        if unsafe { !child.dir()?.subs.unsafe_get().is_empty() } {
            return Err(SysError::ENOTEMPTY);
        }
        let _sub = lk.remove(name).unwrap();
        Ok(())
    }
    pub async fn list(&self) -> SysR<Vec<(DentryType, String)>> {
        let lk = self.subs.shared_lock().await;
        let mut v = Vec::new();
        for (name, inode) in lk.iter() {
            let dt = if inode.is_dir() {
                DentryType::DIR
            } else {
                DentryType::REG
            };
            v.push((dt, name.clone()));
        }
        Ok(v)
    }
    pub fn readable(&self) -> bool {
        self.readable.load(Ordering::Relaxed)
    }
    pub fn writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }
    pub async fn stat(&self, _stat: &mut Stat) -> SysR<()> {
        todo!()
    }
}
