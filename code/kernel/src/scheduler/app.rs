use crate::{scheduler::{add_task_later, self}, task, trap::context::TrapContext};

use super::get_current_idle_cx_ptr;

pub fn suspend_current_and_run_next(trap_context: &mut TrapContext) {
    memory_trace!("suspend_current_and_run_next");
    // DANGER! task can be fetched before schedule!
    add_task_later();
    schedule(trap_context);
    let tcb = trap_context.get_tcb();
    tcb.take_message();
    tcb.handle_message();
}

pub fn exit_current_and_run_next(trap_context: &mut TrapContext, exit_code: i32) -> ! {
    stack_trace!();
    memory_trace!("exit_current_and_run_next");
    unsafe {
        let kernel_stack = trap_context.get_tcb().exit(exit_code);
        scheduler::free_kernel_stack_later(kernel_stack);
        task::goto_task(get_current_idle_cx_ptr());
    };
}

fn schedule(trap_context: &mut TrapContext) {
    memory_trace!("schedule");
    let current_task_cx = trap_context.get_tcb().task_context_ptr();
    let next_cx_ptr = get_current_idle_cx_ptr();
    unsafe { task::switch(current_task_cx, next_cx_ptr) };
}
