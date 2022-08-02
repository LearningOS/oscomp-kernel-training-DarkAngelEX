use alloc::vec::Vec;
use ftl_util::{error::SysRet, time::TimeSpec};

use crate::{
    fs::Pollfd,
    memory::user_ptr::{UserInOutPtr, UserReadPtr},
    signal::SignalSet,
    syscall::Syscall,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

const PRINT_SYSCALL_SELECT: bool = false || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub async fn sys_pselect6(&mut self) -> SysRet {
        // let (nfds, readfds, writefds, exceptfds, timeout, sigmask): () = self.cx.into();
        todo!()
    }
    /// 未实现功能
    pub async fn sys_ppoll(&mut self) -> SysRet {
        stack_trace!();
        let (fds, nfds, timeout, sigmask, s_size): (
            UserInOutPtr<Pollfd>,
            usize,
            UserReadPtr<TimeSpec>,
            UserReadPtr<u8>,
            usize,
        ) = self.cx.into();
        let uc = UserCheck::new(self.process);
        let fds = uc.writable_slice(fds, nfds).await?;
        if PRINT_SYSCALL_SELECT {
            let fds: Vec<_> = fds.access().iter().map(|a| a.fd).collect();
            println!("sys_ppoll fds: {:?} ..", fds);
        }
        let _timeout = match timeout.nonnull() {
            Some(timeout) => Some(uc.readonly_value(timeout).await?.load()),
            None => None,
        };
        let _sigset = if let Some(sigmask) = sigmask.nonnull() {
            let v = uc.readonly_slice(sigmask, s_size).await?;
            SignalSet::from_bytes(&*v.access())
        } else {
            SignalSet::EMPTY
        };
        Ok(nfds)
    }
}
