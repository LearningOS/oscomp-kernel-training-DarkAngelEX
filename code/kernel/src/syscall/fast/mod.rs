use crate::trap::FastStatus;

use super::*;

const SYSCALL_MAX: usize = 400;

/// 返回Err的时候进入async路径
type ENTRY = Option<fn(&mut Syscall<'static>) -> SysRet>;
pub static FAST_SYSCALL_TABLE: [ENTRY; SYSCALL_MAX] = fast_syscall_generate();

const fn fast_syscall_generate() -> [ENTRY; SYSCALL_MAX] {
    let mut table: [ENTRY; SYSCALL_MAX] = [None; SYSCALL_MAX];
    table[SYSCALL_DUP] = Some(Syscall::sys_dup);
    table[SYSCALL_CLOCK_GETTIME] = Some(Syscall::sys_clock_gettime_fast);
    table[SYSCALL_GETRUSAGE] = Some(Syscall::sys_getrusage_fast);
    table[SYSCALL_GETPID] = Some(Syscall::sys_getpid);
    table
}

/// UKContext 中包含 to_executor 标志, 并初始化为 1
pub unsafe fn running_syscall(cx: *mut UKContext) {
    let f = match FAST_SYSCALL_TABLE.get((*cx).a7()).copied() {
        Some(Some(f)) => f,
        Some(None) | None => return,
    };
    let fast_context = (*cx).fast_context();
    let result = f(&mut Syscall::new(
        &mut *cx,
        fast_context.thread_arc,
        fast_context.process,
    ));

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
    match result {
        Ok(a0) => {
            (*cx).set_next_instruction();
            (*cx).set_user_a0(a0);
            (*cx).fast_status = FastStatus::Success;
        }
        Err(_) => return,
    }
}
