use core::arch::global_asm;

use super::context::TaskContext;

type Context = TaskContext;

global_asm!(include_str!("switch.S"));

extern "C" {
    fn __switch(current_task_cx_ptr2: *mut usize, next_task_cx_ptr2: *const usize) -> usize;
}

#[inline(always)]
/// send info
pub unsafe fn switch(current_task_cx: *mut Context, next_task_cx: *const Context) -> usize {
    __switch(current_task_cx as *mut usize, next_task_cx as *const usize)
}
