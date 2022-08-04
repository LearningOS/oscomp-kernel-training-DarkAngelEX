use ftl_util::{
    error::SysError,
    time::{Instant, TimeSpec, TimeVal, TimeZone},
};

use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    timer::{self, ITimerval, Tms},
    user::check::UserCheck,
};

use super::{SysRet, Syscall};

impl Syscall<'_> {
    pub async fn sys_clock_gettime(&mut self) -> SysRet {
        stack_trace!();
        let (_clkid, tp): (usize, UserWritePtr<TimeSpec>) = self.cx.into();
        let cur = TimeSpec::from_instant(timer::now());
        UserCheck::new(self.process)
            .writable_value(tp)
            .await?
            .store(cur);
        Ok(0)
    }
    pub async fn sys_times(&mut self) -> SysRet {
        stack_trace!();
        let ptr: UserWritePtr<Tms> = self.cx.para1();
        if !ptr.is_null() {
            let dst = UserCheck::new(self.process).writable_value(ptr).await?;
            self.thread.timer_fence();
            let timer = self.process.timer.lock();
            let mut tms = Tms::zeroed();
            tms.tms_stime = timer.stime_cur.as_micros() as usize;
            tms.tms_utime = timer.utime_cur.as_micros() as usize;
            tms.tms_cstime = timer.stime_children.as_micros() as usize;
            tms.tms_cutime = timer.utime_children.as_micros() as usize;
            dst.store(tms);
        }
        Ok(timer::now().as_secs() as usize)
    }
    pub async fn sys_gettimeofday(&mut self) -> SysRet {
        stack_trace!();
        let (tv, tz): (UserWritePtr<TimeVal>, UserWritePtr<TimeZone>) = self.cx.into();
        let u_tv = if !tv.is_null() {
            Some(UserCheck::new(self.process).writable_value(tv).await?)
        } else {
            None
        };
        let u_tz = if !tz.is_null() {
            Some(UserCheck::new(self.process).writable_value(tz).await?)
        } else {
            None
        };
        let (tv, tz) = timer::dur_to_tv_tz(timer::now() - Instant::BASE);
        if let Some(p) = u_tv {
            p.store(tv)
        }
        if let Some(p) = u_tz {
            p.store(tz)
        }
        Ok(0)
    }
    pub async fn sys_setitimer(&mut self) -> SysRet {
        let (which, new, old): (usize, UserReadPtr<ITimerval>, UserWritePtr<ITimerval>) =
            self.cx.into();

        let uc = UserCheck::new(self.process);

        let new = uc
            .readonly_value_nullable(new)
            .await?
            .map(|v| v.load().into_duration());
        let old = uc.writable_value_nullable(old).await?;

        // SIGALRM 真实时间
        const ITIMER_REAL: usize = 0;
        // SIGVTALRM 全部线程的用户态时间
        const ITIMER_VIRTUAL: usize = 1;
        // SIGPROF 全部线程的用户态+内核态时间
        const ITIMER_PROF: usize = 2;

        let mut timer = self.process.timer.lock();
        let prev = match which {
            ITIMER_REAL => timer.set_itime_real(new),
            ITIMER_VIRTUAL => timer.set_time_virtual(new),
            ITIMER_PROF => timer.set_time_prof(new),
            _ => return Err(SysError::EINVAL),
        };
        if let Some(old) = old {
            old.store(ITimerval::from_duration(prev))
        }
        Ok(0)
    }
}
