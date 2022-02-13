use crate::{scheduler::add_task_later, task, trap::context::TrapContext};

use super::get_current_idle_cx_ptr;

pub fn suspend_current_and_run_next(trap_context: &mut TrapContext) {
    memory_trace!("suspend_current_and_run_next");
    // DANGER! task can be fetched before schedule!
    add_task_later();
    schedule(trap_context);
}

pub fn exit_current_and_run_next(trap_context: &mut TrapContext, exit_code: i32) -> ! {
    memory_trace!("exit_current_and_run_next");
    unsafe {
        trap_context.get_tcb().exit(exit_code);
        task::goto_task(get_current_idle_cx_ptr());
    };
}

pub fn schedule(trap_context: &mut TrapContext) {
    memory_trace!("schedule");
    let current_task_cx = trap_context.get_tcb().task_context_ptr();
    let next_cx_ptr = get_current_idle_cx_ptr();
    unsafe { task::switch(current_task_cx, next_cx_ptr) };
}
