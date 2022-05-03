use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub fn sys_thread_create(&mut self) -> SysResult {
        stack_trace!();
        let (_entry, _arg): (usize, usize) = self.cx.into();
        let _new_thread = self.process;
        todo!()
    }
}
