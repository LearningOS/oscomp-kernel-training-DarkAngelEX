use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use alloc::{boxed::Box, string::String, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::{
        stat::{Stat, S_IFREG},
        DentryType,
    },
    sync::{rw_sleep_mutex::RwSleepMutex, spin_mutex::SpinMutex, Spin},
    time::{Instant, TimeSpec},
};

use crate::FsInode;

pub struct TmpFsFile {
    readable: AtomicBool,
    writable: AtomicBool,
    subs: RwSleepMutex<Vec<u8>, Spin>,
    timer: SpinMutex<(Instant, Instant), Spin>,
}

impl TmpFsFile {
    pub fn new((r, w): (bool, bool)) -> Self {
        Self {
            readable: AtomicBool::new(r),
            writable: AtomicBool::new(w),
            subs: RwSleepMutex::new(Vec::new()),
            timer: SpinMutex::new((Instant::BASE, Instant::BASE)),
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
        if offset > self.bytes()? {
            return Err(SysError::EINVAL);
        }
        let lk = self.subs.shared_lock().await;
        if offset > self.bytes()? {
            return Err(SysError::EINVAL);
        }
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
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move {
            let (access_time, modify_time) = *self.timer.lock();
            *stat = Stat::zeroed();
            stat.st_dev = 0;
            stat.st_ino = 0;
            stat.st_mode = 0o777;
            stat.st_mode |= S_IFREG;
            stat.st_nlink = 1;
            stat.st_uid = 0;
            stat.st_gid = 0;
            stat.st_rdev = 0;
            stat.st_size = self.bytes().unwrap_or(0);
            stat.st_blksize = 1;
            stat.st_blocks = 0;
            stat.st_atime = access_time.as_secs() as usize;
            stat.st_atime_nsec = access_time.subsec_nanos() as usize;
            stat.st_mtime = modify_time.as_secs() as usize;
            stat.st_mtime_nsec = modify_time.subsec_nanos() as usize;
            stat.st_ctime = modify_time.as_secs() as usize;
            stat.st_ctime_nsec = modify_time.subsec_nanos() as usize;
            Ok(())
        })
    }
    fn utimensat(&self, times: [TimeSpec; 2], now: fn() -> Instant) -> ASysRet {
        Box::pin(async move {
            let [access, modify] = times
                .try_map(|v| v.user_map(now))?
                .map(|v| v.map(|v| v.as_instant()));
            let mut lk = self.timer.lock();
            if let Some(v) = access {
                lk.0 = v;
            }
            if let Some(v) = modify {
                lk.1 = v;
            }
            Ok(0)
        })
    }
    fn list(&self) -> ASysR<Vec<(DentryType, String)>> {
        Box::pin(async move { Err(SysError::ENOTDIR) })
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
        // Arc自动释放
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
