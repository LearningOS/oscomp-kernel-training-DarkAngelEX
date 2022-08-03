use alloc::vec::Vec;
use ftl_util::{async_tools, error::SysRet, time::TimeSpec};
use vfs::select::{SelectFuture, PL};

use crate::{
    fs::Pollfd,
    memory::user_ptr::{UserInOutPtr, UserReadPtr},
    process::fd::Fd,
    signal::SignalSet,
    syscall::Syscall,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

const PRINT_SYSCALL_SELECT: bool = true || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

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
            Some(timeout) => {
                let ts = uc.readonly_value(timeout).await?.load();
                Some(ts.as_duration())
            }
            None => None,
        };
        let _sigset = if let Some(sigmask) = sigmask.nonnull() {
            let v = uc.readonly_slice(sigmask, s_size).await?;
            SignalSet::from_bytes(&*v.access())
        } else {
            SignalSet::EMPTY
        };
        let v = self.alive_then(|a| {
            let mut v = Vec::new();
            for (i, pollfd) in fds.access_mut().iter_mut().enumerate() {
                pollfd.revents = PL::empty();
                if (pollfd.fd as i32) < 0 {
                    continue;
                }
                match a.fd_table.get(Fd(pollfd.fd as usize)) {
                    None => pollfd.revents = PL::POLLNVAL,
                    Some(f) => {
                        let events = pollfd.events;
                        let cur = f.ppoll();
                        if cur.intersects(PL::POLLFAIL | events) {
                            pollfd.revents = cur & (PL::POLLFAIL | events);
                            continue;
                        }
                        v.push((i, f.clone(), cur & PL::POLLSUCCESS));
                    }
                }
            }
            v
        });
        let mut waker = async_tools::take_waker().await;
        let r = SelectFuture::new(v, &mut waker).await;
        let n = r.len();
        for (i, pl) in r {
            fds.access_mut()[i].revents = pl;
        }
        Ok(n)
    }
}
