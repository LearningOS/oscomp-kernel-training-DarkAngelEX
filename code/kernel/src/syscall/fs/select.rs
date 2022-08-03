use alloc::{sync::Arc, vec::Vec};
use bit_field::BitField;
use ftl_util::{
    async_tools,
    error::{SysError, SysR, SysRet},
    time::TimeSpec,
};
use vfs::{
    select::{SelectFuture, PL},
    File,
};

use crate::{
    fs::Pollfd,
    memory::user_ptr::{UserInOutPtr, UserReadPtr},
    process::{fd::Fd, AliveProcess},
    signal::SignalSet,
    syscall::Syscall,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

const PRINT_SYSCALL_SELECT: bool = true || false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub async fn sys_pselect6(&mut self) -> SysRet {
        let (nfds, readfds, writefds, exceptfds, timeout, sigmask): (
            usize,
            UserInOutPtr<usize>,
            UserInOutPtr<usize>,
            UserInOutPtr<usize>,
            UserReadPtr<TimeSpec>,
            UserReadPtr<SignalSet>,
        ) = self.cx.into();
        if nfds == 0 {
            return Err(SysError::EINVAL);
        }
        let arr_n = nfds.div_ceil(usize::BITS as usize);
        let uc = UserCheck::new(self.process);
        let r = uc.writable_slice_nullable(readfds, arr_n).await?;
        let w = uc.writable_slice_nullable(writefds, arr_n).await?;
        let e = uc.writable_slice_nullable(exceptfds, arr_n).await?;
        let timeout = uc
            .readonly_value_nullable(timeout)
            .await?
            .map(|a| a.load().as_duration());

        if PRINT_SYSCALL_SELECT {
            println!(
                "sys_pselect6 nfds: {} r: {:#x} w: {:#x} e: {:#x} timeout: {:?}",
                nfds,
                r.as_ref().map(|v| v.access()[0]).unwrap_or(0),
                w.as_ref().map(|v| v.access()[0]).unwrap_or(0),
                e.as_ref().map(|v| v.access()[0]).unwrap_or(0),
                timeout.map(|a| (a.as_secs(), a.subsec_nanos()))
            );
        }

        let _sigset = uc
            .readonly_value_nullable(sigmask)
            .await?
            .map(|a| a.load())
            .unwrap_or_else(|| *self.thread.inner().signal_manager.mask());

        let mut n = 0;
        let set = self.alive_then(|a| -> SysR<Vec<_>> {
            fn push_set_impl(
                set: &mut Vec<(usize, Arc<dyn File>, PL)>,
                a: &mut AliveProcess,
                ran: &mut [usize],
                events: PL,
            ) -> SysR<usize> {
                let mut n = 0;
                for (i, pv) in ran.iter_mut().enumerate() {
                    let mut v = *pv;
                    let mut r = 0;
                    while v != 0 {
                        let place = v.trailing_zeros() as usize;
                        debug_assert!(v.get_bit(place));
                        v.set_bit(place, false);
                        let fd = i * usize::BITS as usize + place;
                        let f = a.fd_table.get(Fd(fd)).ok_or(SysError::EBADF)?;
                        let cur = f.ppoll();
                        if cur.intersects(PL::POLLFAIL | events) {
                            n += 1;
                            r.set_bit(place, true);
                            continue;
                        }
                        set.push((fd, f.clone(), cur & PL::POLLSUCCESS));
                    }
                    *pv = r;
                }
                Ok(n)
            }
            let mut set = Vec::new();
            if let Some(r) = r.as_ref() {
                n += push_set_impl(&mut set, a, &mut *r.access_mut(), PL::POLLIN)?;
            }
            if let Some(w) = w.as_ref() {
                n += push_set_impl(&mut set, a, &mut *w.access_mut(), PL::POLLOUT)?;
            }
            if let Some(e) = e.as_ref() {
                n += push_set_impl(&mut set, a, &mut *e.access_mut(), PL::POLLPRI)?;
            }
            Ok(set)
        })?;
        if n != 0 || set.is_empty() {
            return Ok(n);
        }
        let mut waker = async_tools::take_waker().await;
        let ret = SelectFuture::new(set, &mut waker).await;
        let n = ret.len();

        let mut r = r.as_ref().map(|v| v.access_mut());
        let mut w = w.as_ref().map(|v| v.access_mut());
        let mut e = e.as_ref().map(|v| v.access_mut());

        let ub = usize::BITS as usize;
        for (i, pl) in ret {
            let x = i / ub;
            let y = i % ub;
            if pl.contains(PL::POLLIN) {
                r.as_mut().unwrap()[x].set_bit(y, true);
            } else if pl.contains(PL::POLLOUT) {
                w.as_mut().unwrap()[x].set_bit(y, true);
            } else if pl.contains(PL::POLLPRI) {
                e.as_mut().unwrap()[x].set_bit(y, true);
            }
        }
        Ok(n)
    }

    pub async fn sys_ppoll(&mut self) -> SysRet {
        stack_trace!();
        let (fds, nfds, timeout, sigmask, s_size): (
            UserInOutPtr<Pollfd>,
            usize,
            UserReadPtr<TimeSpec>,
            UserReadPtr<u8>,
            usize,
        ) = self.cx.into();
        if PRINT_SYSCALL_SELECT {
            println!("sys_ppoll");
        }
        let uc = UserCheck::new(self.process);
        let fds = uc.writable_slice(fds, nfds).await?;
        if PRINT_SYSCALL_SELECT {
            let fds: Vec<_> = fds.access().iter().map(|a| a.fd).collect();
            println!("sys_ppoll fds: {:?} ..", fds);
        }
        let _timeout = uc
            .readonly_value_nullable(timeout)
            .await?
            .map(|a| a.load().as_duration());

        let _sigset = uc
            .readonly_slice_nullable(sigmask, s_size)
            .await?
            .map(|a| SignalSet::from_bytes(&*a.access()))
            .unwrap_or_else(|| *self.thread.inner().signal_manager.mask());

        let mut n = 0;
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
                            n += 1;
                            continue;
                        }
                        v.push((i, f.clone(), cur & PL::POLLSUCCESS));
                    }
                }
            }
            v
        });
        if n != 0 || v.is_empty() {
            return Ok(n);
        }
        let mut waker = async_tools::take_waker().await;
        let r = SelectFuture::new(v, &mut waker).await;
        let n = r.len();
        for (i, pl) in r {
            fds.access_mut()[i].revents = pl;
        }
        Ok(n)
    }
}
