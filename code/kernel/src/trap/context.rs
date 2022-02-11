use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::pin::Pin;

use alloc::sync::Arc;

use crate::memory::address::{KernelAddr4K, UserAddr, UserAddr4K};
use crate::riscv::register::sstatus::{self, Sstatus};
use crate::task::TaskControlBlock;
use crate::user::UserAccessStatus;

/// this trap context is in TCB so it can call drop.
#[repr(C)]
pub struct TrapContext {
    // used in trap.S
    pub x: [usize; 32],                    // regs
    pub sstatus: Sstatus,                  // 32
    pub sepc: UserAddr,                    // 33
    pub kernel_stack: KernelAddr4K,        // 34
    pub kernel_tp: usize,                  // 35
    pub need_add_task: usize,              // 36 add task will set to 1, otherwise 0
    pub new_trap_cx_ptr: *mut TrapContext, // 37
    // pub trap_handler: usize, // unused
    pub tcb: Pin<&'static TaskControlBlock>,
    pub task_new: Option<Arc<TaskControlBlock>>,
    pub user_access_status: UserAccessStatus,
}

unsafe impl Send for TrapContext {}

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
            kernel_tp: uninit(),
            need_add_task: 0,
            new_trap_cx_ptr: core::ptr::null_mut(),
            // trap_handler: uninit(),
            tcb: Pin::new_unchecked(null),
            task_new: None,
            user_access_status: UserAccessStatus::Forbid,
        }
    }
    pub fn sepc(&self) -> UserAddr {
        self.sepc
    }
    pub fn set_sp(&mut self, sp: usize) {
        self.x[2] = sp;
    }
    pub fn set_a0(&mut self, a0: usize) {
        self.x[10] = a0;
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
        let null = unsafe { &*core::ptr::null() };
        Self {
            // x: unsafe { MaybeUninit::uninit().assume_init() },
            x: [0; 32],
            sstatus,
            sepc,
            kernel_stack,
            kernel_tp: 0,
            need_add_task: 0,
            new_trap_cx_ptr: core::ptr::null_mut(),
            // trap_handler: trap::trap_handler as usize,
            tcb: unsafe { Pin::new_unchecked(null) },
            task_new: None,
            user_access_status: UserAccessStatus::Forbid,
        }
    }
    pub fn exec_init(
        &mut self,
        entry: UserAddr,
        user_stack: UserAddr4K,
        kernel_stack: KernelAddr4K,
        tcb: *const TaskControlBlock,
        argc: usize,
        argv: usize,
    ) {
        let sstatus = sstatus::read();
        *self = Self::new(sstatus, entry, kernel_stack);
        self.set_sp(user_stack.into_usize());
        self.set_argc_argv(argc, argv);
        self.set_tcb_ptr(tcb);
    }
    pub fn set_tcb_ptr(&mut self, tcb: *const TaskControlBlock) {
        self.tcb = unsafe { Pin::new_unchecked(&*tcb) }
    }
    pub fn get_tcb(&self) -> &'static TaskControlBlock {
        debug_check!(self.tcb.get_ref() as *const TaskControlBlock as usize != 0);
        self.tcb.get_ref()
    }
    pub unsafe fn get_tcb_mut(&mut self) -> &'static mut TaskControlBlock {
        let tcb = self.get_tcb();
        &mut *{ tcb as *const TaskControlBlock as *mut TaskControlBlock }
    }
    pub fn copy_common_from(&mut self, src: &Self) {
        // ra, sp, gp, tp, sstatus, sepc
        // x1  x2  x3  x4
        self.x[1..=4].copy_from_slice(&src.x[1..=4]);
        self.sstatus = src.sstatus;
        self.sepc = src.sepc;
    }
    pub fn copy_sx_from(&mut self, src: &Self) {
        self.x[8..=9].copy_from_slice(&src.x[8..=9]); // s0-s1
        self.x[18..=27].copy_from_slice(&src.x[18..=27]); // s2-s11
    }
    /// fork for __fork_return
    pub fn fork_no_sx(&self, new_kernel_sp: KernelAddr4K, tcb: *const TaskControlBlock) -> Self {
        // ra, sp, gp, tp, frame, sstatus, sepc, s0-s11, a0
        let mut new = unsafe { Self::any() };
        new.copy_common_from(self);
        // new.copy_sx_from(self);  // should run in xadd_task
        // new.set_a0(0);           // should run in xadd_task
        new.kernel_stack = new_kernel_sp;
        new.need_add_task = 0;
        new.set_tcb_ptr(tcb);
        new
    }
}
