use core::mem::MaybeUninit;
use core::pin::Pin;

use crate::memory::address::{KernelAddr, KernelAddr4K, UserAddr, UserAddr4K};
use crate::riscv::register::sstatus::{self, Sstatus};
use crate::task::TaskControlBlock;
use crate::trap;

#[derive(Debug)]
pub struct TrapContext {
    pub x: [usize; 32],   // regs
    pub sstatus: Sstatus, //
    pub sepc: UserAddr,
    pub kernel_stack: KernelAddr4K,
    // pub trap_handler: usize, // unused
    pub tcb: Pin<&'static TaskControlBlock>,
}

impl TrapContext {
    pub unsafe fn any() -> Self {
        unsafe fn uninit<T>() -> T {
            MaybeUninit::uninit().assume_init()
        }
        #[allow(deref_nullptr)]
        let null = &*core::ptr::null();
        Self {
            x: uninit(),
            sstatus: uninit(),
            sepc: uninit(),
            kernel_stack: uninit(),
            // trap_handler: uninit(),
            tcb: Pin::new_unchecked(null),
        }
    }
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }
    pub fn new(sstatus: Sstatus, sepc: UserAddr, kernel_stack: KernelAddr4K) -> Self {
        #[allow(deref_nullptr)]
        let r = unsafe { &*core::ptr::null() };
        Self {
            // x: unsafe { MaybeUninit::uninit().assume_init() },
            x: [0; 32],
            sstatus,
            sepc,
            kernel_stack,
            // trap_handler: trap::trap_handler as usize,
            tcb: unsafe { Pin::new_unchecked(r) },
        }
    }
    pub fn app_init(entry: UserAddr, user_stack: UserAddr4K, kernel_stack: KernelAddr4K) -> Self {
        let sstatus = sstatus::read();
        let mut cx = Self::new(sstatus, entry, kernel_stack);
        cx.set_sp(user_stack.into_usize());
        cx
    }
    pub fn set_tcb_ptr(&mut self, tcb: *const TaskControlBlock) {
        self.tcb = unsafe { Pin::new_unchecked(&*tcb) }
    }
}
