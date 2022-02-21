use super::{Syscall, SysResult};

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

impl<'a> Syscall<'a> {
    pub fn sys_write(&mut self) -> SysResult {
        todo!()
    }
}
