use core::mem::MaybeUninit;
use core::pin::Pin;

use alloc::sync::Arc;

use crate::memory::address::{KernelAddr4K, UserAddr, UserAddr4K};
use crate::riscv::register::sstatus::{self, Sstatus};
use crate::user::UserAccessStatus;

use super::run_user;

#[repr(C)]
pub struct UKContext {
    pub user_rx: [usize; 32],   // 0-31
    pub user_sepc: UserAddr,    // 32
    pub user_sstatus: Sstatus,  // 33
    pub kernel_sx: [usize; 12], // 34-45
    pub kernel_ra: usize,       // 46
    pub kernel_sp: usize,       // 47
    pub kernel_tp: usize,       // 48
}

impl UKContext {
    pub unsafe fn any() -> Self {
        MaybeUninit::uninit().assume_init()
        // MaybeUninit::zeroed().assume_init()
    }
    pub fn a7(&self) -> usize {
        self.user_rx[17]
    }
    pub fn set_user_sp(&mut self, sp: usize) {
        self.user_rx[2] = sp;
    }
    pub fn set_user_a0(&mut self, a0: usize) {
        self.user_rx[10] = a0;
    }
    pub fn set_argc_argv(&mut self, argc: usize, argv: usize) {
        self.user_rx[10] = argc;
        self.user_rx[11] = argv;
    }
    pub fn syscall_parameter(&self) -> &[usize; 7] {
        let rx = &self.user_rx;
        rx.split_array_ref::<17>().0.rsplit_array_ref().1
    }
    /// sepc += 4
    pub fn into_next_instruction(&mut self) {
        self.user_sepc.add_assign(4);
    }

    pub fn exec_init(
        &mut self,
        user_sp: UserAddr4K,
        sepc: UserAddr,
        sstatus: Sstatus,
        argc: usize,
        argv: usize,
    ) {
        self.set_user_sp(user_sp.into_usize());
        self.set_argc_argv(argc, argv);
        self.user_sepc = sepc;
        self.user_sstatus = sstatus;
    }

    pub fn fork(&self) -> Self {
        let mut new = unsafe { Self::any() };
        new.user_rx = self.user_rx;
        new.user_sepc = self.user_sepc;
        new.user_sstatus = self.user_sstatus;
        new
    }
    pub fn run_user(&mut self) {
        run_user(self)
    }
}
