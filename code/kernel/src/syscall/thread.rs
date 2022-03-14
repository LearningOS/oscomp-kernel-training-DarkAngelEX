use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub fn sys_thread_create(&mut self) -> SysResult {
        let (entry, arg): (usize, usize) = self.cx.parameter2();
    let new_thread = self.process;
        todo!()
    }
}
