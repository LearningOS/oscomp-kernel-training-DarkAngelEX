use core::{
    ops::{Add, AddAssign, Sub, SubAssign},
    time::Duration,
};

use crate::error::{SysError, SysR};

/// 起始时间为 1980-1-1 00:00
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant(Duration);

impl Instant {
    /// 1980-1-1 00:00
    pub const BASE: Self = Instant(Duration::ZERO);
    pub fn year_mount_day_hour_min_second(self) -> (usize, usize, usize, usize, usize, usize) {
        let seconds = self.0.as_secs();
        let mins = seconds / 60;
        let hours = mins / 60;
        let days = hours / 24;
        let mounts = days / 30;
        let years = mounts / 12;
        (
            (years) as usize,
            (mounts - years * 12) as usize,
            (days - mounts / 30) as usize,
            (hours - days * 24) as usize,
            (mins - hours * 60) as usize,
            (seconds - mins * 60) as usize,
        )
    }
    pub fn days(self) -> usize {
        self.year_mount_day_hour_min_second().2
    }
    pub fn subsec_nanos(self) -> u32 {
        self.0.subsec_nanos()
    }
}

impl Add<Duration> for Instant {
    type Output = Self;
    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.0 + rhs)
    }
}
impl Sub<Duration> for Instant {
    type Output = Self;
    fn sub(self, rhs: Duration) -> Self::Output {
        Self(self.0 - rhs)
    }
}
impl Sub for Instant {
    type Output = Duration;
    fn sub(self, rhs: Self) -> Self::Output {
        debug_assert!(self >= rhs);
        self.0 - rhs.0
    }
}
impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, rhs: Duration) {
        self.0 += rhs;
    }
}
impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, rhs: Duration) {
        self.0 -= rhs;
    }
}

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
    pub fn valid(&self) -> SysR<()> {
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
    pub fn as_instant(self) -> Instant {
        Instant::BASE + self.as_duration()
    }
    pub fn user_map(self, now: impl FnOnce() -> Instant) -> SysR<Option<Self>> {
        if self.is_now() {
            Ok(Some(Self::from_duration(now() - Instant::BASE)))
        } else if self.is_omit() {
            Ok(None)
        } else {
            self.valid()?;
            Ok(Some(self))
        }
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
    pub fn from_instant(now: Instant) -> Self {
        let (y, mo, d, h, mi, s) = now.year_mount_day_hour_min_second();
        UtcTime {
            ymd: (y, mo, d),
            hms: (h, mi, s),
            nano: now.subsec_nanos() as usize,
        }
    }
}
