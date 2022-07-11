use ftl_util::time::{TimeSpec, TimeVal, TimeZone};

use crate::{
    memory::user_ptr::UserWritePtr,
    timer::{self, Tms},
    user::check::UserCheck,
};

use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub async fn sys_clock_gettime(&mut self) -> SysResult {
        stack_trace!();
        let (_clkid, tp): (usize, UserWritePtr<TimeSpec>) = self.cx.into();
        let cur = TimeSpec::from_duration(timer::get_time());
        UserCheck::new(self.process)
            .writable_value(tp)
            .await?
            .store(cur);
        Ok(0)
    }
    pub async fn sys_times(&mut self) -> SysResult {
        stack_trace!();
        let ptr: UserWritePtr<Tms> = self.cx.para1();
        if !ptr.is_null() {
            let dst = UserCheck::new(self.process).writable_value(ptr).await?;
            let tms = Tms::zeroed();
            dst.store(tms);
        }
        Ok(timer::get_time().as_secs() as usize)
    }
    pub async fn sys_gettimeofday(&mut self) -> SysResult {
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
        let (tv, tz) = timer::dur_to_tv_tz(timer::get_time());
        u_tv.map(|p| p.store(tv));
        u_tz.map(|p| p.store(tz));
        Ok(0)
    }
}
