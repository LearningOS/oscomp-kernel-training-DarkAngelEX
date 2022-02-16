use alloc::sync::Arc;

use crate::{
    debug::PRINT_SYSCALL,
    loader::get_app_data_by_name,
    memory::allocator::frame,
    riscv::cpu,
    scheduler::{self, app::suspend_current_and_run_next},
    syscall::SYSCALL_FORK,
    task::Pid,
    tools::allocator::from_usize_allocator::FromUsize,
    trap::{context::TrapContext, ADD_TASK_MAGIC},
    user,
};

pub fn sys_getpid(trap_context: &mut TrapContext, _args: [usize; 0]) -> isize {
    stack_trace!();
    trap_context.get_tcb().pid().into_usize() as isize
}

/// [pid, exit_code_ptr]
///
/// pid < -1 wait any process which gpid = |pid|
///
/// pid = -1 wait any subprocess
///
/// pid = 0 wait any process with gpid = self.gpid
///
/// pid > 0 wait any process with pid = pid
///
/// return ret
///
/// ret > 0  pid
///
/// ret = 0  none
///
/// ret = -1 no subprocess
///
/// ret = -2 have running subprocess
///
pub fn sys_waitpid(trap_context: &mut TrapContext, args: [usize; 2]) -> isize {
    stack_trace!();
    memory_trace!("sys_waitpid");
    const PRINT_WAITPID: bool = false;

    if PRINT_SYSCALL || PRINT_WAITPID {
        println!(
            "call sys_waitpid hart: {} {:?}",
            cpu::hart_id(),
            trap_context.get_tcb().pid()
        );
    }
    let pid = args[0] as isize;
    let xpid = Pid::from_usize(pid as usize);
    let exit_code_ptr = args[1] as *mut i32;

    let task = trap_context.get_tcb();

    let zombie = if pid < -1 {
        unimplemented!()
    } else if pid == -1 {
        if task.no_children() {
            return -1;
        }
        task.try_remove_zombie_any()
    } else if pid == 0 {
        unimplemented!()
    } else {
        assert!(pid > 0);
        if !task.have_child_of(xpid) {
            return -1;
        }
        task.try_remove_zombie(xpid)
    };
    let zombie = match zombie {
        Some(zombie) => zombie,
        None => return -2,
    };

    if exit_code_ptr != core::ptr::null_mut() {
        let exit_code = zombie.exit_code();
        let data = match user::translated_user_write_range(trap_context, exit_code_ptr as *mut _, 4)
        {
            Ok(x) => x,
            Err(e) => {
                println!("{:?}", e);
                return -3;
            }
        };
        let write_range = &mut *data.access_mut();
        let src_ptr = &exit_code as *const i32 as *const [u8; 4];
        let src = unsafe { &*src_ptr };
        write_range.copy_from_slice(&src[0..4]);
    }

    // assert_eq!(Arc::strong_count(&zombie), 1);
    if PRINT_WAITPID {
        println!("waitpid recover {:?}", zombie.pid());
    }
    return zombie.pid().into_usize() as isize;
}

pub fn sys_fork(trap_context: &mut TrapContext, _args: [usize; 0]) -> isize {
    stack_trace!();
    memory_trace!("sys_fork");
    if PRINT_SYSCALL {
        println!("call sys_fork hart: {}", cpu::hart_id());
    }
    assert!(trap_context.task_new.is_none());
    let allocator = &mut frame::defualt_allocator();
    match trap_context.get_tcb().fork(allocator) {
        Ok((new_tcb, new_trap_cx_ptr)) => {
            trap_context.new_trap_cx_ptr = new_trap_cx_ptr;
            trap_context.task_new = Some(new_tcb);
            trap_context.need_add_task = ADD_TASK_MAGIC;
            SYSCALL_FORK as isize
        }
        Err(_e) => -1,
    }
}

pub fn sys_exec(trap_context: &mut TrapContext, args: [usize; 2]) -> isize {
    stack_trace!();
    let path = args[0] as *const u8;
    let args = args[1];
    if PRINT_SYSCALL {
        println!(
            "call sys_exec {:?} hart: {}",
            trap_context.get_tcb().pid(),
            cpu::hart_id()
        );
    }
    memory_trace!("sys_exec entry");
    let exec_name = match user::translated_user_str_zero_end(trap_context, path) {
        Ok(str) => str,
        Err(e) => {
            println!("[FTL OS]exec translated user str error: {:?}", e);
            return -1;
        }
    };
    let slice = exec_name.access();
    let str = match core::str::from_utf8(&*slice) {
        Ok(s) => s,
        Err(e) => {
            println!("[FTL OS]exec utf8 error: {}", e);
            return -1;
        }
    };
    println!("exec name: {}", str);
    let elf_data = match get_app_data_by_name(str) {
        Some(data) => data,
        None => {
            println!("[FTL OS]exec name no find.");
            return -1;
        }
    };
    drop(slice);
    let allocator = &mut frame::defualt_allocator();
    let argc = 0;
    let argv = 0;
    println!("unimplement send argc argv");
    match trap_context.get_tcb().exec(elf_data, argc, argv, allocator) {
        Ok(..) => unreachable!(),
        Err(e) => {
            println!("exec error: {:?}", e);
            return -1;
        }
    }
}

pub fn sys_exit(trap_context: &mut TrapContext, args: [usize; 1]) -> ! {
    stack_trace!();
    if PRINT_SYSCALL {
        println!(
            "call sys_exit {:?} hart: {}",
            trap_context.get_tcb().pid(),
            cpu::hart_id()
        );
    }
    let exit_code = args[0] as i32;
    scheduler::app::exit_current_and_run_next(trap_context, exit_code);
}

pub fn sys_yield(trap_context: &mut TrapContext, _args: [usize; 0]) -> isize {
    stack_trace!();
    if PRINT_SYSCALL {
        println!(
            "call sys_yield {:?} hart: {}",
            trap_context.get_tcb().pid(),
            cpu::hart_id()
        );
    }
    scheduler::app::suspend_current_and_run_next(trap_context);
    0
}

pub fn sys_kill(trap_context: &mut TrapContext, args: [usize; 2]) -> isize {
    stack_trace!();
    if PRINT_SYSCALL {
        println!(
            "call sys_kill {:?} hart: {}",
            trap_context.get_tcb().pid(),
            cpu::hart_id()
        );
    }
    todo!()
    // let pid = args[0];
    // let signal = args[1] as u32;
    // if let Some(process) = pid2process(pid) {
    //     if let Some(flag) = SignalFlags::from_bits(signal) {
    //         process.inner_exclusive_access().signals |= flag;
    //         0
    //     } else {
    //         -1
    //     }
    // } else {
    //     -1
    // }
}
