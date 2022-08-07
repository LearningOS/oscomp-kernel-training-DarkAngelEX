use ftl_util::error::{SysError, SysRet};

use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::{
        resource::{self, RLimit, Rusage},
        search, Pid,
    },
    timer,
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
    pub async fn sys_info(&mut self) -> SysRet {
        stack_trace!();
        #[derive(Clone, Copy)]
        struct SysInfo {
            uptime: usize,     /* Seconds since boot */
            loads: [usize; 3], /* 1, 5, and 15 minute load averages */
            totalram: usize,   /* Total usable main memory size */
            freeram: usize,    /* Available memory size */
            sharedram: usize,  /* Amount of shared memory */
            bufferram: usize,  /* Memory used by buffers */
            totalswap: usize,  /* Total swap space size */
            freeswap: usize,   /* Swap space still available */
            procs: u16,        /* Number of current processes */
            totalhigh: usize,  /* Total high memory size */
            freehigh: usize,   /* Available high memory size */
            mem_unit: u32,     /* Memory unit size in bytes */
            _f: [u8; 20 - 2 * core::mem::size_of::<usize>() - core::mem::size_of::<u32>()],
            /* Padding to 64 bytes */
        }
        let info: UserWritePtr<SysInfo> = self.cx.para1();
        if PRINT_SYSCALL_RESOURCE {
            println!("sys_info ptr: {:#x}", info.as_usize(),);
        }
        let ptr = UserCheck::new(self.process).writable_value(info).await?;
        let src = SysInfo {
            uptime: timer::now().as_secs() as usize,
            loads: [0; 3],
            totalram: 0,
            freeram: 0,
            sharedram: 0,
            bufferram: 0,
            totalswap: 0,
            freeswap: 0,
            procs: search::proc_count() as u16,
            totalhigh: 0,
            freehigh: 0,
            mem_unit: 0,
            _f: [0; _],
        };
        ptr.store(src);
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
