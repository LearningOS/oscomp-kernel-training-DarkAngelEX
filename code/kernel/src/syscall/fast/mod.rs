use crate::trap::FastStatus;

use super::*;

const SYSCALL_MAX: usize = 400;

/// 返回Err的时候进入async路径
type Entry = Option<fn(&mut Syscall<'static>) -> SysRet>;
static FAST_SYSCALL_TABLE: [Entry; SYSCALL_MAX] = fast_syscall_generate();

const fn fast_syscall_generate() -> [Entry; SYSCALL_MAX] {
    let mut table: [Entry; SYSCALL_MAX] = [None; SYSCALL_MAX];
    table[SYSCALL_DUP] = Some(Syscall::sys_dup);
    table[SYSCALL_OPENAT] = Some(Syscall::sys_openat_fast);
    table[SYSCALL_CLOSE] = Some(Syscall::sys_close);
    table[SYSCALL_READ] = Some(Syscall::sys_read_fast);
    table[SYSCALL_WRITE] = Some(Syscall::sys_write_fast);
    table[SYSCALL_PSELECT6] = Some(Syscall::sys_pselect6_fast);
    table[SYSCALL_NEWFSTATAT] = Some(Syscall::sys_newfstatat_fast);
    table[SYSCALL_FSTAT] = Some(Syscall::sys_fstat_fast);
    table[SYSCALL_CLOCK_GETTIME] = Some(Syscall::sys_clock_gettime_fast);
    table[SYSCALL_KILL] = Some(Syscall::sys_kill);
    table[SYSCALL_RT_SIGACTION] = Some(Syscall::sys_rt_sigaction_fast);
    table[SYSCALL_GETRUSAGE] = Some(Syscall::sys_getrusage_fast);
    table[SYSCALL_GETPID] = Some(Syscall::sys_getpid);
    table[SYSCALL_GETPPID] = Some(Syscall::sys_getppid);
    table
}

/// UKContext 中包含 to_executor 标志, 并初始化为 1
pub unsafe fn running_syscall(cx: *mut UKContext) {
    let f = match FAST_SYSCALL_TABLE.get((*cx).a7()).copied() {
        Some(Some(f)) => f,
        Some(None) | None => return,
    };
    let fast_context = (*cx).fast_context();
    let mut result;
    {
        let mut call = Syscall::new(&mut *cx, fast_context.thread_arc, fast_context.process);
        result = f(&mut call);
    }
    if !PRINT_SYSCALL_ALL && PRINT_SYSCALL_ERR {
        if let Err(e) = result {
            if !matches!(e, SysError::EAGAIN | SysError::EFAULT) {
                println!(
                    "{}{:?} fast syscall {} -> {:?} sepc:{:#x}{}",
                    to_yellow!(),
                    fast_context.thread.tid(),
                    (*cx).a7(),
                    e,
                    (*cx).user_sepc,
                    reset_color!()
                );
            }
        }
    }

    if PRINT_SYSCALL_ALL {
        // println!("syscall return with {}", a0);
        if PRINT_SYSCALL_RW || ![63, 64].contains(&(*cx).a7()) {
            print!(
                "{}{:?} fast syscall {} -> ",
                to_yellow!(),
                fast_context.thread.tid(),
                (*cx).a7(),
            );
            match result {
                Ok(n) => print!("{:#x} ", n),
                Err(e) => print!("{:?} ", e),
            }
            println!("sepc:{:#x}{}", (*cx).user_sepc, reset_color!());
        }
    }
    // 快速系统调用失败的两种可能
    match result {
        Ok(_) | Err(SysError::EAGAIN) | Err(SysError::EFAULT) => (),
        // 除了EAGAIN和EFAULT之外的其他错误无法被异步路径处理, 直接回用户态
        Err(e) => result = Ok(-(e as isize) as usize),
    }

    if let Ok(a0) = result {
        (*cx).set_next_instruction();
        (*cx).set_user_a0(a0);
        (*cx).fast_status = FastStatus::Success;
    }
}
