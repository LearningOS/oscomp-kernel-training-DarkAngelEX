use core::ops::{Add, AddAssign, Sub, SubAssign};

use ftl_util::error::SysError;

use crate::board::CLOCK_FREQ;
use crate::hart::sbi;

use crate::riscv::register::time;
use crate::xdebug::PRINT_TICK;

pub mod sleep;

/// how many time interrupt per second
const TIME_INTERRUPT_PER_SEC: usize = 20;

pub fn init() {
    sleep::sleep_queue_init();
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct Tms {
    pub tms_utime: usize,  // 当前进程的用户态花费时间
    pub tms_stime: usize,  // 当前进程的内核态花费时间
    pub tms_cutime: usize, // 死去子进程的用户态时间
    pub tms_cstime: usize, // 死去子进程的内核态时间
}

impl Tms {
    pub fn zeroed() -> Self {
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
    pub fn append(&mut self, src: &Self) {
        self.tms_cutime += src.tms_utime + src.tms_cutime;
        self.tms_cstime += src.tms_stime + src.tms_cstime;
    }
    pub fn reset(&mut self) {
        *self = Self::zeroed()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimeSpec {
    pub tv_sec: usize,
    pub tv_nsec: usize, // 纳秒
}

impl TimeSpec {
    pub fn valid(&self) -> Result<(), SysError> {
        if self.tv_nsec >= 1000_000_000 {
            return Err(SysError::EINVAL);
        }
        Ok(())
    }
    pub fn from_ticks(ticks: TimeTicks) -> Self {
        let nsec = ticks.nanosecond();
        TimeSpec {
            tv_sec: (nsec / 1000_000_000) as usize,
            tv_nsec: (nsec % 1000_000_000) as usize,
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
    pub fn from_ticks(ticks: TimeTicks) -> Self {
        let usec = ticks.microsecond();
        Self {
            tv_sec: (usec / 1000_000) as usize,
            tv_usec: (usec % 1000_000) as usize,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct TimeZone {
    tz_minuteswest: u32,
    tz_dsttime: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeTicks(u128);

impl TimeTicks {
    pub const ZERO: Self = Self(0);
    pub const FOREVER: Self = Self(usize::MAX as u128);
    pub fn from_usize(ticks: usize) -> Self {
        Self(ticks as u128)
    }
    pub fn into_usize(self) -> usize {
        self.0 as usize
    }
    pub fn from_time_spec(ts: TimeSpec) -> Self {
        Self::from_second(ts.tv_sec as u128) + Self::from_nanosecond(ts.tv_nsec as u128)
    }
    pub fn time_sepc(self) -> TimeSpec {
        TimeSpec::from_ticks(self)
    }
    /// compute v * m / d in 128bit
    ///
    /// m and d
    #[inline(always)]
    const fn mul_div_128<const M: u128, const D: u128>(v: u128) -> u128 {
        if M >= D {
            if M % D == 0 {
                v * (M / D)
            } else {
                v * M / D
            }
        } else if D % M == 0 {
            v / (D / M)
        } else {
            v * M / D
        }
    }
    #[inline(always)]
    const fn mul_div_128_tick<const M: u128, const D: u128>(v: u128) -> Self {
        Self(Self::mul_div_128::<M, D>(v))
    }
    pub fn from_second(v: u128) -> Self {
        Self::mul_div_128_tick::<CLOCK_FREQ, 1>(v)
    }
    pub fn from_millisecond(v: u128) -> Self {
        Self::mul_div_128_tick::<CLOCK_FREQ, 1000>(v)
    }
    pub fn from_microsecond(v: u128) -> Self {
        Self::mul_div_128_tick::<CLOCK_FREQ, 1000_000>(v)
    }
    pub fn from_nanosecond(v: u128) -> Self {
        Self::mul_div_128_tick::<CLOCK_FREQ, 1000_000_000>(v)
    }
    pub fn second(self) -> u128 {
        Self::mul_div_128::<1, CLOCK_FREQ>(self.0)
    }
    pub fn millisecond(self) -> u128 {
        Self::mul_div_128::<1000, CLOCK_FREQ>(self.0)
    }
    pub fn microsecond(self) -> u128 {
        Self::mul_div_128::<1000_000, CLOCK_FREQ>(self.0)
    }
    pub fn nanosecond(self) -> u128 {
        Self::mul_div_128::<1000_000_000, CLOCK_FREQ>(self.0)
    }
    pub fn into_tv_tz(self) -> (TimeVal, TimeZone) {
        let tv = TimeVal::from_ticks(self);
        let tz = TimeZone {
            tz_minuteswest: 0,
            tz_dsttime: 0,
        };
        (tv, tz)
    }
}
impl Add for TimeTicks {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}
impl AddAssign for TimeTicks {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0
    }
}
impl Sub for TimeTicks {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}
impl SubAssign for TimeTicks {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0
    }
}

impl From<usize> for TimeTicks {
    fn from(ticks: usize) -> Self {
        Self::from_usize(ticks)
    }
}

pub fn get_time_ticks() -> TimeTicks {
    TimeTicks::from_usize(time::read())
}

pub fn set_time_ticks(ticks: TimeTicks) {
    stack_trace!();
    sbi::set_timer(ticks.into_usize() as u64)
}

pub fn set_next_trigger() {
    set_time_ticks(get_time_ticks() + TimeTicks(CLOCK_FREQ / TIME_INTERRUPT_PER_SEC as u128));
}

pub fn tick() {
    if PRINT_TICK {
        print!("!");
    }
    sleep::check_timer();
    set_next_trigger();
}
