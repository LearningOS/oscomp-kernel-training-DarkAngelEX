use core::arch::{asm, global_asm};

use super::context::TaskContext;

global_asm!(include_str!("switch.S"));

extern "C" {
    fn __switch(current_task_cx_ptr2: *const usize, next_task_cx_ptr2: *const usize);
}

#[inline(always)]
pub unsafe fn switch(current_task_cx: *mut TaskContext, next_task_cx: *const TaskContext) {
    __switch(
        current_task_cx as *const usize,
        next_task_cx as *const usize,
    );
}
