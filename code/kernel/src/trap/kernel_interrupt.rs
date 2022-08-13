use riscv::register::{scause, sstatus};

use crate::{local, timer};

#[no_mangle]
pub fn kernel_default_interrupt() {
    stack_trace!();

    debug_assert!(!local::hart_local().interrupt);
    local::hart_local().interrupt = true;
    // 进入陷阱会自动关中断
    debug_assert!(!sstatus::read().sie());

    let interrupt = match scause::read().cause() {
        scause::Trap::Interrupt(i) => i,
        scause::Trap::Exception(e) => {
            panic!("should kernel_interrupt but {:?}", e);
        }
    };

    match interrupt {
        scause::Interrupt::UserSoft => todo!(),
        scause::Interrupt::VirtualSupervisorSoft => todo!(),
        scause::Interrupt::SupervisorSoft => {
            // print!("<{}>", local::hart_local().cpuid());
            local::handle_current_local();
        }
        scause::Interrupt::UserTimer => todo!(),
        scause::Interrupt::VirtualSupervisorTimer => todo!(),
        scause::Interrupt::SupervisorTimer => timer::tick(),
        scause::Interrupt::UserExternal => todo!(),
        scause::Interrupt::VirtualSupervisorExternal => todo!(),
        scause::Interrupt::SupervisorExternal => todo!(),
        scause::Interrupt::Unknown => todo!(),
    }
    local::hart_local().interrupt = false;
}
