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
    pub fn exec_init(&mut self, kernel_sp: KernelAddr4K, trap_context: *const TrapContext) {
        self.ra = trap::exec_return as usize;
        self.sp = kernel_sp.into();
        self.a0 = trap_context as usize;
    }
}
