use core::mem::MaybeUninit;
use core::pin::Pin;

use crate::memory::address::{KernelAddr4K, UserAddr, UserAddr4K};
use crate::riscv::register::sstatus::{self, Sstatus};
use crate::task::TaskControlBlock;

#[repr(C)]
pub struct TrapContext {
    // used in trap.S
    pub x: [usize; 32],   // regs
    pub sstatus: Sstatus, //
    pub sepc: UserAddr,
    pub kernel_stack: KernelAddr4K,
    pub need_add_task: usize, // add task will set to 1, otherwise 0
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
            need_add_task: 0,
            // trap_handler: uninit(),
            tcb: Pin::new_unchecked(null),
        }
    }
    pub fn sepc(&self) -> UserAddr {
        self.sepc
    }
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }
    pub fn set_argc_argv(&mut self, argc: usize, argv: usize) {
        self.x[10] = argc;
        self.x[11] = argv;
    }
    pub fn syscall_parameter(&self) -> (usize, [usize; 3]) {
        let cx = self.x;
        (cx[17], [cx[10], cx[11], cx[12]])
    }
    /// sepc += 4
    pub fn into_next_instruction(&mut self) {
        self.sepc.add_assign(4);
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
            need_add_task: 0,
            // trap_handler: trap::trap_handler as usize,
            tcb: unsafe { Pin::new_unchecked(r) },
        }
    }
    pub fn exec_init(
        &mut self,
        entry: UserAddr,
        user_stack: UserAddr4K,
        kernel_stack: KernelAddr4K,
        argc: usize,
        argv: usize,
    ) {
        let sstatus = sstatus::read();
        *self = Self::new(sstatus, entry, kernel_stack);
        self.set_sp(user_stack.into_usize());
        self.set_argc_argv(argc, argv);
    }
    pub fn set_tcb_ptr(&mut self, tcb: *const TaskControlBlock) {
        self.tcb = unsafe { Pin::new_unchecked(&*tcb) }
    }
}
