use super::{SysRet, Syscall};

impl Syscall<'_> {
    pub fn sys_gettid(&mut self) -> SysRet {
        Ok(self.thread.tid().0)
    }
}
