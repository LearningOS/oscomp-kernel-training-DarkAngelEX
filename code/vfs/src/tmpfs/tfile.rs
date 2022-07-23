use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::stat::Stat,
    sync::{rw_sleep_mutex::RwSleepMutex, Spin},
};

use crate::FsInode;

pub struct TmpFsFile {
    readable: AtomicBool,
    writable: AtomicBool,
    subs: RwSleepMutex<Vec<u8>, Spin>,
}

impl TmpFsFile {
    pub fn new((r, w): (bool, bool)) -> Self {
        Self {
            readable: AtomicBool::new(r),
            writable: AtomicBool::new(w),
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
impl FsInode for TmpFsFile {
    fn readable(&self) -> bool {
        self.readable.load(Ordering::Relaxed)
    }
    fn writable(&self) -> bool {
        self.writable.load(Ordering::Relaxed)
    }
    fn is_dir(&self) -> bool {
        false
    }
    fn stat<'a>(&'a self, _stat: &'a mut Stat) -> ASysR<()> {
        todo!()
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
    fn unlink_child<'a>(&'a self, _name: &'a str, _release: bool) -> ASysR<()> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn rmdir_child<'a>(&'a self, _name: &'a str) -> ASysR<()> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
    }
    fn bytes(&self) -> SysRet {
        self.bytes()
    }
    fn reset_data(&self) -> ASysR<()> {
        Box::pin(async move { self.reset_data().await })
    }
    fn delete(&self) {
        todo!()
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        (offset, ptr): (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            let n = self.read_at(offset, buf).await?;
            if let Some(ptr) = ptr {
                ptr.store(offset + n, Ordering::Release);
            }
            Ok(n)
        })
    }
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        (offset, ptr): (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet {
        Box::pin(async move {
            let n = self.write_at(offset, buf).await?;
            if let Some(ptr) = ptr {
                ptr.store(offset + n, Ordering::Release);
            }
            Ok(n)
        })
    }
}
