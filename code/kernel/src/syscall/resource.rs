use ftl_util::error::{SysError, SysRet};

use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::{
        resource::{self, RLimit, Rusage},
        search, Pid,
    },
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::Syscall;

const PRINT_SYSCALL_RESOURCE: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub fn sys_getrusage_fast(&mut self) -> SysRet {
        let (who, usage): (u32, UserWritePtr<Rusage>) = self.cx.into();
        if PRINT_SYSCALL_RESOURCE {
            println!(
                "sys_getrusage_fast who: {} usage ptr: {:#x}",
                who as isize,
                usage.as_usize()
            );
        }
        let usage = UserCheck::writable_value_only(usage)?;
        self.thread.timer_fence();
        usage.access_mut()[0].write(who, self.thread)?;
        Ok(0)
    }
    pub async fn sys_getrusage(&mut self) -> SysRet {
        let (who, usage): (u32, UserWritePtr<Rusage>) = self.cx.into();
        if PRINT_SYSCALL_RESOURCE {
            println!(
                "sys_getrusage who: {} usage ptr: {:#x}",
                who as isize,
                usage.as_usize()
            );
        }
        let usage = UserCheck::new(self.process).writable_value(usage).await?;
        self.thread.timer_fence();
        usage.access_mut()[0].write(who, self.thread)?;
        Ok(0)
    }
    /// 设置系统资源
    pub async fn sys_prlimit64(&mut self) -> SysRet {
        stack_trace!();
        let (pid, resource, new_limit, old_limit): (
            Pid,
            u32,
            UserReadPtr<RLimit>,
            UserWritePtr<RLimit>,
        ) = self.cx.into();

        if PRINT_SYSCALL_RESOURCE {
            println!(
                "sys_prlimit64 pid:{:?}, resource:{}, new_ptr: {:#x}, old_ptr: {:#x}",
                pid,
                resource,
                new_limit.as_usize(),
                old_limit.as_usize()
            );
        }

        let uc = UserCheck::new(self.process);
        let new = match new_limit.is_null() {
            false => Some(uc.readonly_value(new_limit).await?.load()),
            true => None,
        };
        if (PRINT_SYSCALL_RESOURCE || false) && let Some(new) = new {
                println!("new: {:?}", new);
            }
        let old = match pid {
            Pid(0) => resource::prlimit_impl(self.process, resource, new)?,
            _ => {
                let process = search::find_proc(pid).ok_or(SysError::ESRCH)?;
                resource::prlimit_impl(&process, resource, new)?
            }
        };
        if !old_limit.is_null() {
            uc.writable_value(old_limit).await?.store(old);
        }
        Ok(0)
    }
}
