use super::{SysResult, Syscall};

impl Syscall<'_> {
    pub fn sys_thread_create(&mut self) -> SysResult {
        let (_entry, _arg): (usize, usize) = self.cx.para2();
    let _new_thread = self.process;
        todo!()
    }
}
