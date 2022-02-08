use core::arch::global_asm;

use self::context::TrapContext;

pub mod context;
global_asm!(include_str!("trap.S"));

/// return value is sscratch = ptr of TrapContext
#[no_mangle]
pub fn trap_handler() -> usize {
    todo!()
}

#[no_mangle]
pub fn trap_from_kernel() -> ! {
    panic!("a trap from kernel!");
}

#[inline(always)]
pub fn trap_return(trap_context: &TrapContext) -> ! {
    extern "C" {
        fn __trap_return(a0: usize) -> !;
    }
    unsafe { __trap_return(trap_context as *const TrapContext as usize) }
}
