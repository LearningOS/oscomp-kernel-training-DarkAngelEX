use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub fn sys_gettid(&mut self) -> SysResult {
        Ok(self.thread.tid().0)
    }
}
