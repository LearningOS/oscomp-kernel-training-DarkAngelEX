use alloc::sync::Arc;
use lazy_static::__Deref;

use crate::{
    loader::get_app_data_by_name,
    memory::allocator::frame,
    scheduler,
    syscall::SYSCALL_FORK,
    trap::{context::TrapContext, ADD_TASK_MAGIC},
    user,
};

pub fn sys_waitpid(trap_context: &mut TrapContext, args: [usize; 2]) -> isize {
    let pid = args[0] as isize;
    let exit_code_ptr = args[1] as *mut i32;
    let mut task = trap_context.get_tcb().lock();
    let children = task.get_children();
    if pid != -1
        && !children
            .iter()
            .any(|p| p.pid().into_usize() == pid as usize)
    {
        return -1;
    }
    let pair = children
        .iter()
        .enumerate()
        .find(|(_, p)| p.is_zombie() && (pid == -1 || p.pid().into_usize() == pid as usize));

    if let Some((idx, _x)) = pair {
        let child = children.remove(idx);
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.pid();
        // ++++ temporarily access child TCB exclusively
        let exit_code = child.exit_code();
        // ++++ release child PCB
        let data = match user::translated_user_write_range(trap_context, exit_code_ptr as *mut _, 4)
        {
            Ok(x) => x,
            Err(e) => {
                println!("{:?}", e);
                return -2;
            }
        };
        let write_range = &mut *data.access_mut();
        let src_ptr = &exit_code as *const i32 as *const [u8; 4];
        let src = unsafe { &*src_ptr };
        write_range.copy_from_slice(&src[0..4]);

        found_pid.into_usize() as isize
    } else {
        -2
    }
}

pub fn sys_fork(trap_context: &mut TrapContext, _args: [usize; 0]) -> isize {
    println!("call sys_fork");
    memory_trace!("sys_fork");
    assert!(trap_context.task_new.is_none());
    let allocator = &mut frame::defualt_allocator();
    match trap_context.get_tcb().fork(allocator) {
        Ok(new_tcb) => {
            trap_context.new_trap_cx_ptr = new_tcb.trap_context_ptr();
            trap_context.task_new = Some(new_tcb);
            trap_context.need_add_task = ADD_TASK_MAGIC;
            SYSCALL_FORK as isize
        }
        Err(_e) => -1,
    }
}

pub fn sys_exec(trap_context: &mut TrapContext, args: [usize; 1]) -> isize {
    let path = args[0] as *const u8;
    println!("call sys_exec");
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
    let exit_code = args[0] as i32;
    scheduler::app::exit_current_and_run_next(trap_context, exit_code);
}
