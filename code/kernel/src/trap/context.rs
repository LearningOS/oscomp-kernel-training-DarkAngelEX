use core::mem::MaybeUninit;

use crate::memory::address::{KernelAddr, KernelAddr4K, UserAddr, UserAddr4K, VirAddr};
use crate::riscv::register::sstatus::{self, Sstatus};
use crate::trap;
pub struct TrapContext {
    pub x: [usize; 32],   // regs
    pub sstatus: Sstatus, //
    pub sepc: UserAddr,
    pub kernel_stack: KernelAddr,
    pub trap_handler: usize, // unused
}

impl TrapContext {
    pub unsafe fn any() -> Self {
        MaybeUninit::uninit().assume_init()
    }
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }
    pub fn new(sstatus: Sstatus, sepc: UserAddr, kernel_stack: KernelAddr) -> Self {
        Self {
            // x: unsafe { MaybeUninit::uninit().assume_init() },
            x: [0; 32],
            sstatus,
            sepc,
            kernel_stack,
            trap_handler: trap::trap_handler as usize,
        }
    }
    pub fn app_init(entry: UserAddr, user_stack: UserAddr4K, kernel_stack: KernelAddr4K) -> Self {
        let sstatus = sstatus::read();
        let mut cx = Self::new(sstatus, entry, KernelAddr::from(kernel_stack).into());
        cx.set_sp(user_stack.into_usize());
        cx
    }
}
