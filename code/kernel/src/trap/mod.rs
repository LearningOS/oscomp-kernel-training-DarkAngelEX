use core::arch::global_asm;

use crate::{
    riscv::{
        cpu,
        register::{
            scause::{self, Exception, Interrupt},
            sie, stval,
            stvec::{self, TrapMode},
        },
    },
    syscall,
};

use self::context::TrapContext;

pub mod context;
global_asm!(include_str!("trap.S"));

pub fn init() {
    println!("[FTL OS]trap init");
    set_kernel_trap_entry();
}

#[inline(always)]
pub fn get_trap_entry() -> usize {
    extern "C" {
        fn __trap_entry();
    }
    __trap_entry as usize
}
#[no_mangle]
pub fn fast_syscall_handler(
    trap_context: &mut TrapContext,
    // a0 in trap
    a1: usize,
    a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    a6: usize,
    a7: usize,
) -> (isize, &mut TrapContext) {
    set_kernel_trap_entry();
    trap_context.into_next_instruction();
    let a0 = a6;
    let ret = syscall::syscall(a7, [a0, a1, a2]);
    // todo!("fast_syscall_handler");
    before_trap_return();
    (ret, trap_context)
}
/// return value is sscratch = ptr of TrapContext
#[no_mangle]
pub fn trap_handler(trap_context: &mut TrapContext) -> &mut TrapContext {
    set_kernel_trap_entry();
    let scause = scause::read();
    let stval = stval::read();
    match scause.cause() {
        scause::Trap::Exception(e) => match e {
            Exception::UserEnvCall => {
                // system call
                trap_context.into_next_instruction();
                let (a7, paras) = trap_context.syscall_parameter();
                let ret = syscall::syscall(a7, paras);
                trap_context.set_a0(ret as usize);
            }
            Exception::StoreFault
            | Exception::StorePageFault
            | Exception::LoadFault
            | Exception::LoadPageFault
            | Exception::InstructionFault
            | Exception::InstructionPageFault => {
                println!(
                "[kernel] {:?} in application, bad addr = {:#x}, bad instruction = {:?}, kernel killed it.",
                scause.cause(),
                stval,
                trap_context.sepc(),
            );
                panic!();
            }
            Exception::InstructionMisaligned => todo!(),
            Exception::IllegalInstruction => todo!(),
            Exception::Breakpoint => todo!(),
            Exception::StoreMisaligned => todo!(),
            Exception::VirtualSupervisorEnvCall => todo!(),
            Exception::InstructionGuestPageFault => todo!(),
            Exception::LoadGuestPageFault => todo!(),
            Exception::VirtualInstruction => todo!(),
            Exception::StoreGuestPageFault => todo!(),
            Exception::Unknown => todo!(),
        },
        scause::Trap::Interrupt(e) => match e {
            Interrupt::UserSoft => todo!(),
            Interrupt::VirtualSupervisorSoft => todo!(),
            Interrupt::SupervisorSoft => todo!(),
            Interrupt::UserTimer => todo!(),
            Interrupt::VirtualSupervisorTimer => todo!(),
            Interrupt::SupervisorTimer => todo!(),
            Interrupt::UserExternal => todo!(),
            Interrupt::VirtualSupervisorExternal => todo!(),
            Interrupt::SupervisorExternal => todo!(),
            Interrupt::Unknown => todo!(),
        },
    }
    before_trap_return();
    trap_context
}

#[no_mangle]
pub fn trap_from_kernel() -> ! {
    panic!(
        "a trap {:?} from kernel! hart = {}",
        scause::read().cause(),
        cpu::hart_id()
    );
}

#[inline(always)]
pub fn before_trap_return() {
    set_user_trap_entry();
}

#[inline(always)]
pub fn trap_return(trap_context: &mut TrapContext) -> ! {
    extern "C" {
        fn __trap_return(a0: usize) -> !;
    }
    before_trap_return();
    unsafe { __trap_return(trap_context as *mut TrapContext as usize) }
}

fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

fn set_user_trap_entry() {
    unsafe {
        stvec::write(get_trap_entry(), TrapMode::Direct);
    }
}

pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}
