use core::time::Duration;

use crate::error::SysError;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimeSpec {
    pub tv_sec: usize,
    pub tv_nsec: usize, // 纳秒
}

impl TimeSpec {
    pub const UTIME_NOW: usize = (1 << 30) - 1;
    pub const UTIME_OMIT: usize = (1 << 30) - 2;
    pub const NOW: Self = TimeSpec {
        tv_sec: 0,
        tv_nsec: Self::UTIME_NOW,
    };
    pub const OMIT: Self = TimeSpec {
        tv_sec: 0,
        tv_nsec: Self::UTIME_OMIT,
    };
    pub const fn is_now(self) -> bool {
        self.tv_nsec == Self::UTIME_NOW
    }
    pub const fn is_omit(self) -> bool {
        self.tv_nsec == Self::UTIME_OMIT
    }
    pub fn valid(&self) -> Result<(), SysError> {
        if self.tv_nsec >= 1000_000_000 {
            return Err(SysError::EINVAL);
        }
        Ok(())
    }
    pub fn from_duration(dur: Duration) -> Self {
        TimeSpec {
            tv_sec: dur.as_secs() as usize,
            tv_nsec: dur.subsec_nanos() as usize,
        }
    }
    pub fn as_duration(self) -> Duration {
        Duration::from_nanos(self.tv_nsec as u64) + Duration::from_secs(self.tv_sec as u64)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimeVal {
    pub tv_sec: usize,
    pub tv_usec: usize, // 微妙
}
impl TimeVal {
    pub fn from_duration(dur: Duration) -> Self {
        Self {
            tv_sec: dur.as_secs() as usize,
            tv_usec: dur.subsec_micros() as usize,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimeZone {
    pub tz_minuteswest: u32,
    pub tz_dsttime: u32,
}

pub struct UtcTime {
    pub ymd: (usize, usize, usize),
    pub hms: (usize, usize, usize),
    pub nano: usize,
}

impl UtcTime {
    pub fn base() -> Self {
        let mut v: Self = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        v.ymd.0 = 1980;
        v
    }
    pub fn set_ymd(&mut self, ymd: u16) {
        let year = (ymd as usize >> 9) + 1980;
        let mount = (ymd as usize) >> 5 & ((1 << 4) - 1);
        let day = ymd as usize & ((1 << 5) - 1);
        self.ymd = (year, mount, day);
    }
    pub fn set_hms(&mut self, hms: u16) {
        let hour = (hms as usize >> 11).min(23);
        let minute = ((hms as usize) >> 5 & ((1 << 6) - 1)).min(59);
        let second = ((hms as usize & ((1 << 5) - 1)) * 2).min(59);
        self.hms = (hour, minute, second);
    }
    pub fn set_ms(&mut self, ms: u8) {
        self.nano = ms as usize * 1000 * 1000;
    }
    pub fn second(&self) -> usize {
        let mut cur = (self.ymd.0 - 1980) * 365 * 24 * 3600;
        cur += self.ymd.1 * 30 * 24 * 3600;
        cur += self.ymd.2 * 24 * 3600;
        cur += self.hms.0 * 3600;
        cur += self.hms.1 * 60;
        cur += self.hms.2;
        cur
    }
    pub fn nanosecond(&self) -> usize {
        self.nano
    }
}
