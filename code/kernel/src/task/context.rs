use core::mem::MaybeUninit;

use crate::{
    memory::address::{KernelAddr, KernelAddr4K},
    trap::{self, context::TrapContext},
};

/// switch in kernel space
#[repr(C)]
pub struct TaskContext {
    s: [usize; 12],
    ra: usize,
    sp: KernelAddr,
    a0: usize, // return value
}

impl TaskContext {
    pub unsafe fn any() -> Self {
        MaybeUninit::uninit().assume_init()
    }
    pub fn goto_trap_return(kernel_sp: KernelAddr4K, trap_context: *const TrapContext) -> Self {
        Self {
            s: unsafe { MaybeUninit::uninit().assume_init() },
            ra: trap::trap_return as usize,
            sp: kernel_sp.into(),
            a0: trap_context as usize,
        }
    }
}
