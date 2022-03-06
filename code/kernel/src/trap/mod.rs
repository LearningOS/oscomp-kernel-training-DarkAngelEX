use core::arch::{asm, global_asm};

use crate::hart::{self, cpu};
use crate::riscv::register::{
    scause, sie, stval,
    stvec::{self, TrapMode},
};
use crate::timer;

use self::context::UKContext;

pub mod context;

global_asm!(include_str!("trap.S"));

pub fn init() {
    println!("[FTL OS]trap init");
    unsafe { set_kernel_trap_entry() };
}

#[inline(always)]
pub fn run_user(cx: &mut UKContext) {
    extern "C" {
        fn __entry_user(cx: *mut UKContext);
    }
    unsafe {
        set_user_trap_entry();
        __entry_user(cx);
        set_kernel_trap_entry();
    };
}

#[inline(always)]
pub fn get_return_from_user() -> usize {
    extern "C" {
        fn __return_from_user();
    }
    __return_from_user as usize
}

#[no_mangle]
pub fn trap_from_kernel() {
    unsafe { close_kernel_trap_entry() };
    memory_trace_show!("trap_from_kernel entry");
    // println!("trap_from_kernel {:?} sepc = {:#x}", scause::read().cause(), get_sepc());
    match scause::read().cause() {
        scause::Trap::Exception(e) => match e {
            scause::Exception::InstructionMisaligned => todo!(),
            scause::Exception::InstructionFault => todo!(),
            scause::Exception::IllegalInstruction => todo!(),
            scause::Exception::Breakpoint => todo!(),
            scause::Exception::LoadFault => todo!(),
            scause::Exception::StoreMisaligned => todo!(),
            scause::Exception::StoreFault => todo!(),
            scause::Exception::UserEnvCall => todo!(),
            scause::Exception::VirtualSupervisorEnvCall => todo!(),
            scause::Exception::InstructionPageFault => fatal_error(),
            scause::Exception::LoadPageFault => fatal_error(),
            scause::Exception::StorePageFault => fatal_error(),
            scause::Exception::InstructionGuestPageFault => todo!(),
            scause::Exception::LoadGuestPageFault => todo!(),
            scause::Exception::VirtualInstruction => todo!(),
            scause::Exception::StoreGuestPageFault => todo!(),
            scause::Exception::Unknown => fatal_error(),
        },
        scause::Trap::Interrupt(i) => match i {
            scause::Interrupt::UserSoft => todo!(),
            scause::Interrupt::VirtualSupervisorSoft => todo!(),
            scause::Interrupt::SupervisorSoft => todo!(),
            scause::Interrupt::UserTimer => todo!(),
            scause::Interrupt::VirtualSupervisorTimer => todo!(),
            scause::Interrupt::SupervisorTimer => timer::tick(),
            scause::Interrupt::UserExternal => todo!(),
            scause::Interrupt::VirtualSupervisorExternal => todo!(),
            scause::Interrupt::SupervisorExternal => todo!(),
            scause::Interrupt::Unknown => fatal_error(),
        },
    }
    unsafe { set_kernel_trap_entry() };
    return;

    fn get_sepc() -> usize {
        let sepc;
        unsafe {
            asm!("csrr {}, sepc", out(reg)sepc);
        }
        sepc
    }
    fn fatal_error() -> ! {
        let sepc = get_sepc();
        panic!(
            "a trap {:?} from kernel! bad addr = {:#x}, sepc = {:#x}, hart = {} sp = {:#x}",
            scause::read().cause(),
            stval::read(),
            sepc,
            cpu::hart_id(),
            hart::current_sp(),
        );
    }
}
#[no_mangle]
pub fn loop_forever() -> ! {
    println!("entry loop_forever");
    loop {}
}

pub unsafe fn set_kernel_trap_entry() {
    extern "C" {
        fn __kernel_trap_entry();
    }
    stvec::write(__kernel_trap_entry as usize, TrapMode::Direct);
}

pub unsafe fn close_kernel_trap_entry() {
    stvec::write(loop_forever as usize, TrapMode::Direct);
}

#[inline(always)]
unsafe fn set_user_trap_entry() {
    stvec::write(get_return_from_user(), TrapMode::Direct);
}

pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}
