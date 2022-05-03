use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    syscall::SysError,
    user::check::UserCheck,
};

use super::{SysResult, Syscall};

const SIG_BLOCK: usize = 1;
const SIG_UNBLOCK: usize = 2;
const SIG_SETMASK: usize = 3;

bitflags! {
    struct SA: u32 {
        const ONSTACK = 0x00000001;
        const RESTART = 0x00000002;
        const NOCLDSTOP = 0x00000004;
        const NODEFER = 0x00000008;
        const RESETHAND = 0x00000010;
        const NOCLDWAIT = 0x00000020;
        const SIGINFO = 0x00000040;
    }
}

impl Syscall<'_> {
    /// 仅修改当前线程 mask 等价于 pthread_sigmask
    pub async fn sys_rt_sigprocmask(&mut self) -> SysResult {
        stack_trace!();
        // s_size is bytes
        let (how, newset, oldset, s_size): (usize, UserReadPtr<u8>, UserWritePtr<u8>, usize) =
            self.cx.into();
        if true {
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
            9.. => return Err(SysError::EINVAL),
            _ => (),
        }
        let user_check = UserCheck::new(self.process);
        if newset.as_uptr_nullable().ok_or(SysError::EINVAL)?.is_null() {
            return Ok(0);
        }
        let newset = user_check
            .translated_user_readonly_slice(newset, s_size)
            .await?;
        let newset = &*newset.access();
        let sig_mask = &mut self.thread.inner().signal_mask;
        let old = *sig_mask;
        match how {
            SIG_BLOCK => sig_mask.set_bit(newset),
            SIG_UNBLOCK => sig_mask.clear_bit(newset),
            SIG_SETMASK => sig_mask.set(newset),
            _ => return Err(SysError::EINVAL),
        }
        if let Some(oldset) = oldset.nonnull_mut() {
            let v = user_check
                .translated_user_writable_slice(oldset, s_size)
                .await?;
            old.write_to(&mut *v.access_mut());
        }
        Ok(0)
    }
    pub async fn sys_rt_sigaction(&mut self) -> SysResult {
        stack_trace!();
        let (sig, _new_act, _old_act, _s_size): (usize, UserReadPtr<u8>, UserWritePtr<u8>, usize) =
            self.cx.into();
        if sig >= 32 {
            return Err(SysError::EINVAL);
        }
        todo!()
    }
}
