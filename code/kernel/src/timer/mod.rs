use core::ops::{Add, AddAssign, Sub, SubAssign};

use crate::board::CLOCK_FREQ;
use crate::hart::sbi;

use crate::riscv::register::time;

pub mod sleep;

/// how many time interrupt per second
const TIME_INTERRUPT_PER_SEC: usize = 20;
const MILLISECOND_PER_SEC: usize = 1000;

pub fn init() {
    sleep::sleep_queue_init();
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TimeTicks(usize);

impl TimeTicks {
    pub fn from_usize(ticks: usize) -> Self {
        Self(ticks)
    }
    pub fn into_usize(self) -> usize {
        self.0
    }
    pub fn from_millisecond(ms: usize) -> Self {
        Self(ms * (CLOCK_FREQ / MILLISECOND_PER_SEC))
    }
    pub fn into_millisecond(self) -> usize {
        self.0 / (CLOCK_FREQ / MILLISECOND_PER_SEC)
    }
    pub fn into_second(self) -> usize {
        self.0 / CLOCK_FREQ
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
    set_time_ticks(get_time_ticks() + TimeTicks::from(CLOCK_FREQ / TIME_INTERRUPT_PER_SEC));
}

pub fn tick() {
    sleep::check_timer();
    set_next_trigger();
}
