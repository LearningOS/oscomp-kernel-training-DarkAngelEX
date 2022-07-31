use core::time::Duration;

use ftl_util::{
    error::{SysError, SysR},
    time::{Instant, TimeVal},
};

use crate::config::USER_STACK_SIZE;

use super::{thread::Thread, Process};

pub const RLIM_INFINITY: usize = i32::MAX as usize;
const _STK_LIM: u32 = 8 * 1024 * 1024;

const RLIMIT_CPU: u32 = 0;
const RLIMIT_FSIZE: u32 = 1;
const RLIMIT_DATA: u32 = 2;
const RLIMIT_STACK: u32 = 3;
const RLIMIT_CORE: u32 = 4;
const RLIMIT_RSS: u32 = 5;
const RLIMIT_NPROC: u32 = 6;
const RLIMIT_NOFILE: u32 = 7;
const RLIMIT_MEMLOCK: u32 = 8;
const RLIMIT_AS: u32 = 9;
const RLIMIT_LOCKS: u32 = 10;
const RLIMIT_SIGPENDING: u32 = 11;
const RLIMIT_MSGQUEUE: u32 = 12;
const RLIMIT_NICE: u32 = 13;
const RLIMIT_RTPRIO: u32 = 14;
const RLIMIT_RTTIME: u32 = 15;
const RLIM_NLIMITS: u32 = 16;

const RUSAGE_SELF: u32 = 0;
const RUSAGE_CHILDREN: u32 = u32::MAX;
const RUSAGE_THREAD: u32 = 1;

#[derive(Clone, Copy)]
pub struct ProcessTimer {
    pub utime_cur: Duration,
    pub stime_cur: Duration,
    pub utime_children: Duration,
    pub stime_children: Duration,
}

impl ProcessTimer {
    pub const ZERO: Self = Self {
        utime_cur: Duration::ZERO,
        stime_cur: Duration::ZERO,
        utime_children: Duration::ZERO,
        stime_children: Duration::ZERO,
    };
    pub fn append_child(&mut self, child: &Self) {
        self.utime_children += child.utime_cur + child.utime_children;
        self.stime_children += child.stime_cur + child.stime_children;
    }
}

pub struct ThreadTimer {
    pub utime_submit: Duration,     // 提交到进程的用户态时间
    pub stime_submit: Duration,     // 提交到进程的内核态时间
    pub utime: Duration,            // 未提交的用户态花费的时间
    pub stime: Duration,            // 未提交的内核态花费的时间
    pub time_point: Instant,        // 计时点
    pub time_point_submit: Instant, // 上次将时间提交至进程的时间
    pub max_diff: Duration,         // 提交给进程的最大时间间隔
    pub running: bool,
    pub user: bool,
}

impl ThreadTimer {
    pub const ZERO: Self = Self {
        utime_submit: Duration::ZERO,
        stime_submit: Duration::ZERO,
        utime: Duration::ZERO,
        stime: Duration::ZERO,
        time_point: Instant::BASE,
        time_point_submit: Instant::BASE,
        max_diff: Duration::from_millis(5),
        running: false,
        user: false,
    };
    pub fn utime(&self) -> Duration {
        self.utime_submit + self.utime
    }
    pub fn stime(&self) -> Duration {
        self.stime_submit + self.stime
    }
    pub fn enter_user(&mut self, instant: Instant) {
        debug_assert!(self.time_point != Instant::BASE);
        debug_assert!(self.running && !self.user);
        self.stime += instant - self.time_point;
        self.time_point = instant;
        self.user = true;
    }
    pub fn leave_user(&mut self, instant: Instant) {
        debug_assert!(self.running && self.user);
        self.utime += instant - self.time_point;
        self.time_point = instant;
        self.user = false;
    }
    pub fn enter_thread(&mut self, instant: Instant) {
        debug_assert!(!self.running && !self.user);
        self.time_point = instant;
        self.running = true;
    }
    pub fn leave_thread(&mut self, instant: Instant) {
        debug_assert!(self.running && !self.user);
        self.stime += instant - self.time_point;
        self.time_point = instant;
        self.running = false;
    }
    pub fn timer_fence(&mut self, instant: Instant) {
        debug_assert!(self.running && !self.user);
        self.stime += instant - self.time_point;
        self.time_point = instant;
    }
    /// 将时间提交给进程
    pub fn submit(&mut self, dst: &mut ProcessTimer) {
        dst.utime_cur += self.utime;
        dst.stime_cur += self.stime;
        self.utime_submit += self.utime;
        self.stime_submit += self.stime;
        self.utime = Duration::ZERO;
        self.stime = Duration::ZERO;
        self.time_point_submit = self.time_point;
    }
    pub fn maybe_submit(&mut self, dst: &Process) {
        if self.time_point_submit + self.max_diff < self.time_point {
            return;
        }
        self.submit(&mut *dst.timer.lock());
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RLimit {
    pub rlim_cur: usize, /* Soft limit */
    pub rlim_max: usize, /* Hard limit (ceiling for rlim_cur) */
}

impl RLimit {
    pub const INFINITY: Self = Self::new(RLIM_INFINITY, RLIM_INFINITY);
    pub const fn new(cur: usize, max: usize) -> Self {
        Self {
            rlim_cur: cur,
            rlim_max: max,
        }
    }
    pub const fn new_equal(n: usize) -> Self {
        Self {
            rlim_cur: n,
            rlim_max: n,
        }
    }
    pub fn check(self) -> SysR<()> {
        (self.rlim_cur <= RLIM_INFINITY && self.rlim_max <= RLIM_INFINITY)
            .then_some(())
            .ok_or(SysError::EINVAL)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Rusage {
    pub ru_utime: TimeVal,  // user CPU time used
    pub ru_stime: TimeVal,  // system CPU time used
    pub ru_maxrss: usize,   // maximum resident set size
    pub ru_ixrss: usize,    // integral shared memory size
    pub ru_idrss: usize,    // integral unshared data size
    pub ru_isrss: usize,    // integral unshared stack size
    pub ru_minflt: usize,   // page reclaims (soft page faults)
    pub ru_majflt: usize,   // page faults (hard page faults)
    pub ru_nswap: usize,    // swaps */
    pub ru_inblock: usize,  // block input operations
    pub ru_oublock: usize,  // block output operations
    pub ru_msgsnd: usize,   // IPC messages sent
    pub ru_msgrcv: usize,   // IPC messages received
    pub ru_nsignals: usize, // signals received
    pub ru_nvcsw: usize,    // voluntary context switches
    pub ru_nivcsw: usize,   // involuntary context switches
}

impl Rusage {
    pub const ZERO: Self = Self {
        ru_utime: TimeVal::ZERO,
        ru_stime: TimeVal::ZERO,
        ru_maxrss: 0,
        ru_ixrss: 0,
        ru_idrss: 0,
        ru_isrss: 0,
        ru_minflt: 0,
        ru_majflt: 0,
        ru_nswap: 0,
        ru_inblock: 0,
        ru_oublock: 0,
        ru_msgsnd: 0,
        ru_msgrcv: 0,
        ru_nsignals: 0,
        ru_nvcsw: 0,
        ru_nivcsw: 0,
    };
    pub fn write(&mut self, who: u32, thread: &Thread) -> SysR<()> {
        *self = Self::ZERO;
        match who {
            RUSAGE_SELF => {
                let timer = thread.process.timer.lock();
                self.ru_utime = timer.utime_cur.into();
                self.ru_stime = timer.stime_cur.into();
            }
            RUSAGE_CHILDREN => {
                let timer = thread.process.timer.lock();
                self.ru_utime = timer.utime_children.into();
                self.ru_stime = timer.stime_children.into();
            }
            RUSAGE_THREAD => {
                self.ru_utime = thread.timer().utime().into();
                self.ru_stime = thread.timer().stime().into();
            }
            _ => return Err(SysError::EINVAL),
        }
        Ok(())
    }
}

pub fn prlimit_impl(proc: &Process, resource: u32, new: Option<RLimit>) -> SysR<RLimit> {
    match resource {
        RLIMIT_CPU => todo!(),
        RLIMIT_FSIZE => todo!(),
        RLIMIT_DATA => todo!(),
        RLIMIT_STACK => {
            // debug_assert!(new.is_none());
            Ok(RLimit::new(USER_STACK_SIZE, RLIM_INFINITY))
        }
        RLIMIT_CORE => todo!(),
        RLIMIT_RSS => todo!(),
        RLIMIT_NPROC => todo!(),
        RLIMIT_NOFILE => Ok(proc.alive_then(|a| a.fd_table.set_limit(new))??),
        RLIMIT_MEMLOCK => todo!(),
        RLIMIT_AS => todo!(),
        RLIMIT_LOCKS => todo!(),
        RLIMIT_SIGPENDING => todo!(),
        RLIMIT_MSGQUEUE => todo!(),
        RLIMIT_NICE => todo!(),
        RLIMIT_RTPRIO => todo!(),
        RLIMIT_RTTIME => todo!(),
        RLIM_NLIMITS => todo!(),
        _ => Err(SysError::EINVAL),
    }
}
