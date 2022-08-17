use core::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, Ordering},
};

use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use ftl_util::{
    container::str_map::StrMap,
    error::{SysError, SysR},
    fs::{
        stat::{Stat, S_IFDIR},
        DentryType,
    },
    sync::{rw_sleep_mutex::RwSleepMutex, Spin},
};

use crate::FsInode;

use super::{TmpFs, TmpFsInode};

pub struct TmpFsDir {
    readable: AtomicBool,
    writable: AtomicBool,
    subs: RwSleepMutex<StrMap<TmpFsInode>, Spin>,
    ino: usize,
    fs: NonNull<TmpFs>,
}
unsafe impl Send for TmpFsDir {}
unsafe impl Sync for TmpFsDir {}

impl TmpFsDir {
    pub(super) fn new((r, w): (bool, bool), ino: usize, fs: NonNull<TmpFs>) -> Self {
        Self {
            readable: AtomicBool::new(r),
            writable: AtomicBool::new(w),
            subs: RwSleepMutex::new(StrMap::new()),
            ino,
            fs,
        }
    }
    #[allow(clippy::cast_ref_to_mut)]
    pub(super) unsafe fn set_fs(&self, fs: NonNull<TmpFs>) {
        *(&self.fs as *const _ as *mut _) = fs;
    }
    pub fn search_fast(&self, name: &str) -> SysR<Box<dyn FsInode>> {
        let lk = self.subs.try_shared_lock().ok_or(SysError::EAGAIN)?;
        let d = lk.get(name).ok_or(SysError::ENOENT)?.clone();
        Ok(Box::new(d))
    }
    pub async fn search(&self, name: &str) -> SysR<Box<dyn FsInode>> {
        let lk = self.subs.shared_lock().await;
        let d = lk.get(name).ok_or(SysError::ENOENT)?.clone();
        Ok(Box::new(d))
    }
    pub fn create_fast(&self, name: &str, dir: bool, rw: (bool, bool)) -> SysR<Box<dyn FsInode>> {
        let mut lk = self.subs.try_unique_lock().ok_or(SysError::EAGAIN)?;
        if lk.get(name).is_some() {
            return Err(SysError::EEXIST);
        }
        let ino = unsafe { (*self.fs.as_ptr()).inoalloc.fetch_add(1, Ordering::Relaxed) };
        let new = TmpFsInode::new(dir, rw, ino, self.fs);
        lk.force_insert(name.to_string(), new.clone());
        Ok(Box::new(new))
    }
    pub async fn create(&self, name: &str, dir: bool, rw: (bool, bool)) -> SysR<Box<dyn FsInode>> {
        let mut lk = self.subs.unique_lock().await;
        if lk.get(name).is_some() {
            return Err(SysError::EEXIST);
        }
        let ino = unsafe { (*self.fs.as_ptr()).inoalloc.fetch_add(1, Ordering::Relaxed) };
        let new = TmpFsInode::new(dir, rw, ino, self.fs);
        lk.force_insert(name.to_string(), new.clone());
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
        lk.force_insert(name.to_string(), new.clone());
        Ok(Box::new(new))
    }
    pub async fn unlink_child<'a>(&'a self, name: &'a str, _release: bool) -> SysR<()> {
        let mut lk = self.subs.unique_lock().await;
        let sub = lk.get(name).ok_or(SysError::ENOENT)?;
        if sub.is_dir() {
            return Err(SysError::EISDIR);
        }
        let _f = lk.force_remove(name);
        Ok(())
    }
    pub async fn rmdir_child<'a>(&'a self, name: &'a str) -> SysR<()> {
        let mut lk = self.subs.unique_lock().await;
        let child = lk.get(name).ok_or(SysError::ENOENT)?;
        if unsafe { !child.dir()?.subs.unsafe_get().is_empty() } {
            return Err(SysError::ENOTEMPTY);
        }
        let _sub = lk.force_remove(name);
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
    pub fn dev_ino(&self) -> (usize, usize) {
        (unsafe { (*self.fs.as_ptr()).dev }, self.ino)
    }
    pub fn stat_fast(&self, stat: &mut Stat) -> SysR<()> {
        *stat = Stat::zeroed();
        stat.st_dev = unsafe { (*self.fs.as_ptr()).dev as u64 };
        stat.st_ino = self.ino as u64;
        stat.st_mode = 0o777;
        stat.st_mode |= S_IFDIR;
        stat.st_nlink = 1;
        stat.st_uid = 0;
        stat.st_gid = 0;
        stat.st_rdev = 0;
        stat.st_size = 4096;
        Ok(())
    }
    pub async fn stat(&self, stat: &mut Stat) -> SysR<()> {
        self.stat_fast(stat)
    }
}
