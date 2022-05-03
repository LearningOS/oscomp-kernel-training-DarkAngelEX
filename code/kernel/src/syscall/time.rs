use crate::{
    memory::user_ptr::UserWritePtr,
    timer::{self, Tms},
    user::check::UserCheck,
};

use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub async fn sys_times(&mut self) -> SysResult {
        stack_trace!();
        let ptr: UserWritePtr<Tms> = self.cx.para1();
        if !ptr.is_null() {
            let dst = UserCheck::new(self.process)
                .translated_user_writable_value(ptr)
                .await?;
            let tms = Tms::zeroed();
            dst.store(tms);
        }
        Ok(timer::get_time_ticks().into_second())
    }
    pub fn sys_gettime(&mut self) -> SysResult {
        stack_trace!();
        let time = timer::get_time_ticks().into_millisecond();
        Ok(time)
    }
}
