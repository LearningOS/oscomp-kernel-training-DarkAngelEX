use core::{
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alloc::{boxed::Box, string::String, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    faster,
    fs::{
        stat::{Stat, S_IFREG},
        DentryType,
    },
    sync::{rw_sleep_mutex::RwSleepMutex, spin_mutex::SpinMutex, Spin},
    time::{Instant, TimeSpec},
};

use crate::{select::PL, FsInode};

use super::TmpFs;

pub struct TmpFsFile {
    readable: AtomicBool,
    writable: AtomicBool,
    subs: RwSleepMutex<Vec<u8>, Spin>,
    timer: SpinMutex<(Instant, Instant), Spin>,
    ino: usize,
    fs: NonNull<TmpFs>,
}

unsafe impl Send for TmpFsFile {}
unsafe impl Sync for TmpFsFile {}

impl TmpFsFile {
    pub(super) fn new((_r, w): (bool, bool), ino: usize, fs: NonNull<TmpFs>) -> Self {
        Self {
            readable: AtomicBool::new(true),
            writable: AtomicBool::new(w),
            subs: RwSleepMutex::new(Vec::new()),
            timer: SpinMutex::new((Instant::BASE, Instant::BASE)),
            ino,
            fs,
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
    fn read_at_fast(&self, offset: usize, buf: &mut [u8]) -> SysRet {
        if offset > self.bytes()? {
            return Err(SysError::EINVAL);
        }
        let lk = self.subs.try_shared_lock().ok_or(SysError::EAGAIN)?;
        if offset > self.bytes()? {
            return Err(SysError::EINVAL);
        }
        let end = lk.len().min(offset + buf.len());
        let n = end - offset;
        faster::u8copy(&mut buf[..n], &lk[offset..end]);
        Ok(n)
    }
    fn write_at_fast(&self, offset: usize, buf: &[u8]) -> SysRet {
        let expand = offset + buf.len() > self.bytes()?;
        if !expand {
            let lk = self.subs.try_shared_lock().ok_or(SysError::EAGAIN)?;
            let end = offset + buf.len();
            if end <= lk.len() {
                unsafe {
                    // ??????????????????????????????
                    #[allow(clippy::cast_ref_to_mut)]
                    (*(&lk[offset..end] as *const _ as *mut [u8])).copy_from_slice(buf);
                }
                return Ok(buf.len());
            }
        }
        let mut lk = self.subs.try_unique_lock().ok_or(SysError::EAGAIN)?;
        let end = offset + buf.len();
        if end > lk.len() {
            // ????????????????????????????????????
            #[allow(clippy::uninit_assumed_init)]
            lk.resize(end, unsafe {
                core::mem::MaybeUninit::uninit().assume_init()
            });
        }
        lk[offset..end].copy_from_slice(buf);
        Ok(buf.len())
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
                    // ??????????????????????????????
                    #[allow(clippy::cast_ref_to_mut)]
                    (*(&lk[offset..end] as *const _ as *mut [u8])).copy_from_slice(buf);
                }
                return Ok(buf.len());
            }
        }
        let mut lk = self.subs.unique_lock().await;
        let end = offset + buf.len();
        if end > lk.len() {
            // ????????????????????????????????????
            #[allow(clippy::uninit_assumed_init)]
            lk.resize(end, unsafe {
                core::mem::MaybeUninit::uninit().assume_init()
            });
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
    fn ppoll(&self) -> PL {
        if self.bytes().unwrap() != 0 {
            PL::POLLIN | PL::POLLOUT
        } else {
            PL::POLLOUT
        }
    }
    fn dev_ino(&self) -> (usize, usize) {
        (unsafe { (*self.fs.as_ptr()).dev }, self.ino)
    }
    fn stat_fast(&self, stat: &mut Stat) -> SysR<()> {
        let (access_time, modify_time) = *self.timer.lock();
        *stat = Stat::zeroed();
        stat.st_dev = unsafe { (*self.fs.as_ptr()).dev as u64 };
        stat.st_ino = self.ino as u64;
        stat.st_mode = 0o777;
        stat.st_mode |= S_IFREG;
        stat.st_nlink = 1;
        stat.st_uid = 0;
        stat.st_gid = 0;
        stat.st_rdev = 0;
        stat.st_size = self.bytes().unwrap();
        stat.st_blksize = 512;
        stat.st_blocks = 0;
        stat.st_atime = access_time.as_secs() as usize;
        stat.st_atime_nsec = access_time.subsec_nanos() as usize;
        stat.st_mtime = modify_time.as_secs() as usize;
        stat.st_mtime_nsec = modify_time.subsec_nanos() as usize;
        stat.st_ctime = modify_time.as_secs() as usize;
        stat.st_ctime_nsec = modify_time.subsec_nanos() as usize;
        Ok(())
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()> {
        Box::pin(async move { self.stat_fast(stat) })
    }
    fn utimensat(&self, times: [TimeSpec; 2], now: fn() -> Instant) -> ASysR<()> {
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
            Ok(())
        })
    }
    fn detach(&self) -> ASysR<()> {
        Box::pin(async move { Ok(()) })
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
    fn read_at_fast(&self, buf: &mut [u8], (offset, ptr): (usize, Option<&AtomicUsize>)) -> SysRet {
        let n = self.read_at_fast(offset, buf)?;
        if let Some(ptr) = ptr {
            ptr.store(offset + n, Ordering::Release);
        }
        Ok(n)
    }
    fn write_at_fast(&self, buf: &[u8], (offset, ptr): (usize, Option<&AtomicUsize>)) -> SysRet {
        let n = self.write_at_fast(offset, buf)?;
        if let Some(ptr) = ptr {
            ptr.store(offset + n, Ordering::Release);
        }
        Ok(n)
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
