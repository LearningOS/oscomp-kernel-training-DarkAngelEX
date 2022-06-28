use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    signal::{SigAction, SignalSet, SIG_N, SIG_N_BYTES},
    syscall::SysError,
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_SIGNAL: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

const SIG_BLOCK: usize = 1;
const SIG_UNBLOCK: usize = 2;
const SIG_SETMASK: usize = 3;

bitflags! {
    struct SA: u32 {
        const ONSTACK   = 0x00000001;
        const RESTART   = 0x00000002;
        const NOCLDSTOP = 0x00000004;
        const NODEFER   = 0x00000008;
        const RESETHAND = 0x00000010;
        const NOCLDWAIT = 0x00000020;
        const SIGINFO   = 0x00000040;
    }
}

impl Syscall<'_> {
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
        if sig >= SIG_N as u32 {
            return Err(SysError::EINVAL);
        }
        let manager = &self.process.signal_manager;
        let user_check = UserCheck::new(self.process);
        if new_act
            .as_uptr_nullable()
            .ok_or(SysError::EINVAL)?
            .is_null()
        {
            if let Some(old_act) = old_act.nonnull_mut() {
                let old = manager.get_sig_action(sig);
                user_check
                    .translated_user_writable_value(old_act)
                    .await?
                    .store(old);
            }
            return Ok(0);
        }
        let new_act = user_check
            .translated_user_readonly_value(new_act)
            .await?
            .load();
        assert!(new_act.restorer != 0); // 目前没有映射sigreturn
        if PRINT_SYSCALL_SIGNAL {
            new_act.show();
        }
        let mut old = SigAction::zeroed();
        manager.replace_action(sig, &new_act, &mut old);
        if let Some(old_act) = old_act.nonnull_mut() {
            user_check
                .translated_user_writable_value(old_act)
                .await?
                .store(old);
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
        match s_size {
            0 => return Ok(0),
            SIG_N_BYTES.. => return Err(SysError::EINVAL),
            _ => (),
        }
        let manager = &mut self.thread.inner().signal_manager;
        let sig_mask = manager.mask_mut();
        let user_check = UserCheck::new(self.process);
        if let Some(oldset) = oldset.nonnull_mut() {
            let v = user_check
                .translated_user_writable_slice(oldset, s_size)
                .await?;
            sig_mask.write_to(&mut *v.access_mut());
        }
        if newset.as_uptr_nullable().ok_or(SysError::EINVAL)?.is_null() {
            return Ok(0);
        }
        let newset = user_check
            .translated_user_readonly_slice(newset, s_size)
            .await?;
        let newset = SignalSet::from_bytes(&*newset.access());
        match how {
            SIG_BLOCK => sig_mask.insert(&newset),
            SIG_UNBLOCK => sig_mask.remove(&newset),
            SIG_SETMASK => *sig_mask = newset,
            _ => return Err(SysError::EINVAL),
        }
        Ok(0)
    }
    pub async fn sys_rt_sigpending(&mut self) -> SysResult {
        todo!()
    }
    pub async fn sys_rt_sigtimedwait(&mut self) -> SysResult {
        todo!()
    }
    pub async fn sys_rt_sigqueueinfo(&mut self) -> SysResult {
        todo!()
    }
    pub async fn sys_rt_sigreturn(&mut self) -> SysResult {
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
