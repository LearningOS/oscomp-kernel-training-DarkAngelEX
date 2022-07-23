use super::{SysRet, Syscall};

impl Syscall<'_> {
    pub fn sys_gettid(&mut self) -> SysRet {
        Ok(self.thread.tid().0)
    }
    pub fn sys_membarrier(&mut self) -> SysRet {
        // 内存屏障目前未使用
        Ok(0)
    }
}
