use crate::timer;

use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub fn sys_gettime(&mut self) -> SysResult {
        stack_trace!();
        let time = timer::get_time_ticks().into_millisecond();
        Ok(time)
    }
}
