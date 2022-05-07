use riscv::register::{scause, sstatus};

use crate::{local, timer};

// 中断已经被关闭。
#[no_mangle]
pub fn kernel_default_interrupt() {
    stack_trace!();
    let interrupt = match scause::read().cause() {
        scause::Trap::Interrupt(i) => i,
        scause::Trap::Exception(e) => {
            panic!("should kernel_interrupt but {:?}", e);
        }
    };
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
