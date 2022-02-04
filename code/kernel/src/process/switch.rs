use core::arch::global_asm;

use super::context::SwitchContext;

type Context = SwitchContext;

global_asm!(include_str!("switch.S"));

extern "C" {
    fn __switch(current_task_cx_ptr2: *const usize, next_task_cx_ptr2: *const usize);
}

#[inline(always)]
pub unsafe fn switch(current_task_cx: &mut Context, next_task_cx: &Context) {
    __switch(
        current_task_cx as *mut Context as *const usize,
        next_task_cx as *const Context as *const usize,
    );
}
