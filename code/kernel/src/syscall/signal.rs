use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::{search, Pid, Tid},
    signal::{Sig, SigAction, SignalSet, SignalStack, SIG_N},
    sync::even_bus::Event,
    syscall::SysError,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_SIGNAL: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL || false;

const SIG_BLOCK: usize = 0;
const SIG_UNBLOCK: usize = 1;
const SIG_SETMASK: usize = 2;

bitflags! {
    struct SA: u32 {
        const NOCLDSTOP = 0x00000001;
        const NOCLDWAIT = 0x00000002;
        const SIGINFO   = 0x00000004;
        const RESTORER  = 0x04000000;
        const ONSTACK   = 0x08000000;
        const RESTART   = 0x10000000;
        const NODEFER   = 0x40000000;
        const RESETHAND = 0x80000000;
    }
}

impl Syscall<'_> {
    pub fn sys_kill(&mut self) -> SysResult {
        stack_trace!();
        let (pid, signal): (isize, u32) = self.cx.into();

        if PRINT_SYSCALL_SIGNAL {
            println!("sys_kill pid:{} signal:{}", pid, signal);
        }

        if signal == 0 {
            unimplemented!();
        }
        let signal = Sig::from_user(signal)?;

        enum Target {
            Pid(Pid),     // > 0
            AllInGroup,   // == 0
            All,          // == -1 all have authority except initproc
            Group(usize), // < -1
        }

        let target = match pid {
            0 => Target::AllInGroup,
            -1 => Target::All,
            p if p > 0 => Target::Pid(Pid(p as usize)),
            g => Target::Group(-g as usize),
        };
        match target {
            Target::Pid(pid) => {
                let proc = search::find_proc(pid).ok_or(SysError::ESRCH)?;
                proc.signal_manager.receive(signal);
                proc.event_bus
                    .set(Event::RECEIVE_SIGNAL)?;
            }
            Target::AllInGroup => todo!(),
            Target::All => todo!(),
            Target::Group(_) => todo!(),
        }
        Ok(0)
    }
    pub fn sys_tkill(&mut self) -> SysResult {
        stack_trace!();
        let (tid, sig): (Tid, u32) = self.cx.into();
        if PRINT_SYSCALL_SIGNAL {
            println!("sys_tkill tid: {:?} signal: {}", tid, sig);
        }
        let thread = search::find_thread(tid).ok_or(SysError::ESRCH)?;
        if sig != 0 {
            thread.receive(Sig::from_user(sig)?);
        }
        Ok(0)
    }
    pub fn sys_tgkill(&mut self) -> SysResult {
        stack_trace!();
        let (pid, tid, signal): (Pid, Tid, u32) = self.cx.into();
        if PRINT_SYSCALL_SIGNAL {
            println!("sys_tgkill pid:{:?} tid:{:?} signal:{}", pid, tid, signal);
        }
        let thread = search::find_thread(tid).ok_or(SysError::ESRCH)?;
        if thread.process.pid() != pid {
            return Err(SysError::ESRCH);
        }
        if signal != 0 {
            thread.receive(Sig::from_user(signal)?);
        }
        Ok(0)
    }
    pub async fn sys_sigaltstack(&mut self) -> SysResult {
        stack_trace!();
        /* Structure describing a signal stack.  */
        let (new, old): (UserReadPtr<SignalStack>, UserWritePtr<SignalStack>) = self.cx.into();
        if PRINT_SYSCALL_SIGNAL {
            println!(
                "sys_sigaltstack new:{:#x} old:{:#x}",
                new.as_usize(),
                old.as_usize()
            );
        }
        let _new = UserCheck::new(self.process)
            .readonly_value(new)
            .await?
            .load();

        todo!()
    }
    pub async fn sys_rt_sigsuspend(&mut self) -> SysResult {
        todo!()
    }
    /// 设置信号行为
    ///
    pub async fn sys_rt_sigaction(&mut self) -> SysResult {
        stack_trace!();
        let (sig, new_act, old_act, s_size): (
            u32,
            UserReadPtr<SigAction>,
            UserWritePtr<SigAction>,
            usize,
        ) = self.cx.into();
        if PRINT_SYSCALL_SIGNAL {
            println!(
                "sys_rt_sigaction sig:{} new_act:{:#x} old_act:{:#x} s_size:{}",
                sig,
                new_act.as_usize(),
                old_act.as_usize(),
                s_size
            );
        }
        let sig = Sig::from_user(sig)?;
        debug_assert!(s_size <= SIG_N);
        let manager = &self.process.signal_manager;
        let user_check = UserCheck::new(self.process);
        if new_act
            .as_uptr_nullable()
            .ok_or(SysError::EINVAL)?
            .is_null()
        {
            if let Some(old_act) = old_act.nonnull_mut() {
                let old = manager.get_sig_action(sig);
                user_check.writable_value(old_act).await?.store(*old);
            }
            return Ok(0);
        }
        let new_act = user_check.readonly_value(new_act).await?.load();
        assert!(new_act.restorer != 0); // 目前没有映射sigreturn
        if PRINT_SYSCALL_SIGNAL {
            new_act.show();
        }
        let mut old = SigAction::zeroed();
        manager.replace_action(sig, &new_act, &mut old);
        if let Some(old_act) = old_act.nonnull_mut() {
            user_check.writable_value(old_act).await?.store(old);
        }
        Ok(0)
    }
    /// 设置信号阻塞位并返回原先值
    ///
    /// 仅修改当前线程 mask 等价于 pthread_sigmask
    pub async fn sys_rt_sigprocmask(&mut self) -> SysResult {
        stack_trace!();
        // s_size is bytes
        let (how, newset, oldset, s_size): (usize, UserReadPtr<u8>, UserWritePtr<u8>, usize) =
            self.cx.into();
        if PRINT_SYSCALL_SIGNAL {
            println!(
                "sys_rt_sigprocmask how:{:#x} newset:{:#x} oldset:{:#x} s_size:{}",
                how,
                newset.as_usize(),
                oldset.as_usize(),
                s_size
            );
        }
        debug_assert!(s_size <= SIG_N);
        let manager = &mut self.thread.inner().signal_manager;
        let sig_mask = manager.mask_mut();
        if PRINT_SYSCALL_SIGNAL {
            println!("old: {:#x}", sig_mask.0[0]);
        }
        let user_check = UserCheck::new(self.process);
        if let Some(oldset) = oldset.nonnull_mut() {
            let v = user_check.writable_slice(oldset, s_size).await?;
            sig_mask.write_to(&mut *v.access_mut());
        }
        if newset.as_uptr_nullable().ok_or(SysError::EINVAL)?.is_null() {
            return Ok(0);
        }
        let newset = user_check.readonly_slice(newset, s_size).await?;
        let newset = SignalSet::from_bytes(&*newset.access());
        match how {
            SIG_BLOCK => sig_mask.insert(&newset),
            SIG_UNBLOCK => sig_mask.remove(&newset),
            SIG_SETMASK => *sig_mask = newset,
            _ => return Err(SysError::EINVAL),
        }
        if PRINT_SYSCALL_SIGNAL {
            println!("new: {:#x?}", sig_mask.0[0]);
        }
        Ok(0)
    }
    pub async fn sys_rt_sigpending(&mut self) -> SysResult {
        todo!()
    }
    pub async fn sys_rt_sigtimedwait(&mut self) -> SysResult {
        // todo!()
        Ok(0)
    }
    pub async fn sys_rt_sigqueueinfo(&mut self) -> SysResult {
        todo!()
    }
    pub async fn sys_rt_sigreturn(&mut self) -> SysResult {
        if PRINT_SYSCALL_SIGNAL {
            println!("sys_rt_sigreturn");
        }
        if self.thread.inner().scx_ptr.is_null() {
            self.do_exit = true;
            return Err(SysError::EPERM);
        }
        match crate::signal::sigreturn(self.thread.inner(), self.process).await {
            Ok(a0) => Ok(a0),
            Err(e) => {
                self.do_exit = true;
                Err(e)
            }
        }
    }
}
