use riscv::register::{
    scause::{self, Interrupt},
    sstatus,
};

use crate::{local, timer};

pub fn kernel_default_interrupt(interrupt: Interrupt) {
    stack_trace!();
    if cfg!(debug_assertions) {
        let it = &mut local::hart_local().interrupt;
        assert!(!*it);
        *it = true;
        assert!(!sstatus::read().sie());
    }
    match interrupt {
        scause::Interrupt::UserSoft => todo!(),
        scause::Interrupt::VirtualSupervisorSoft => todo!(),
        scause::Interrupt::SupervisorSoft => todo!(),
        scause::Interrupt::UserTimer => todo!(),
        scause::Interrupt::VirtualSupervisorTimer => todo!(),
        scause::Interrupt::SupervisorTimer => timer::tick(),
        scause::Interrupt::UserExternal => todo!(),
        scause::Interrupt::VirtualSupervisorExternal => todo!(),
        scause::Interrupt::SupervisorExternal => todo!(),
        scause::Interrupt::Unknown => todo!(),
    }
    if cfg!(debug_assertions) {
        local::hart_local().interrupt = false;
    }
}
