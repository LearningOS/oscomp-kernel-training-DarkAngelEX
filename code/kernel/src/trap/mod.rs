use core::arch::global_asm;

// ASCII: Goto Task
#[allow(unused)]
pub const ADD_TASK_MAGIC: usize = 0x476F746F_5461736B;

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
// add_task will return 0xf
#[no_mangle]
pub fn syscall_handler(
    trap_context: &mut TrapContext,
    // a0 in trap
    a1: usize,
    a2: usize,
    _a3: usize,
    _a4: usize,
    _a5: usize,
    a0: usize,
    a7: usize,
) -> (isize, &mut TrapContext) {
    set_kernel_trap_entry();
    assert!(trap_context.need_add_task == 0);
    trap_context.into_next_instruction();
    let ret = syscall::syscall(a7, [a0, a1, a2]);
    // todo!("syscall_handler");
    if trap_context.need_add_task == 0 {
        before_trap_return();
    } else {
        debug_check!(trap_context.need_add_task == ADD_TASK_MAGIC);
    }
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
                // // system call
                // trap_context.into_next_instruction();
                // let (a7, paras) = trap_context.syscall_parameter();
                // let ret = syscall::syscall(a7, paras);
                // trap_context.set_a0(ret as usize);
                panic!("should into syscall_handler!");
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
pub fn trap_add_task(a0: isize, trap_context: &mut TrapContext) -> (isize, &mut TrapContext) {
    assert_eq!(trap_context.need_add_task, ADD_TASK_MAGIC);
    (a0, trap_context)
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
pub fn exec_return(trap_context: &mut TrapContext) -> ! {
    extern "C" {
        fn __exec_return(a0: usize) -> !;
    }
    before_trap_return();
    unsafe { __exec_return(trap_context as *mut TrapContext as usize) }
}

#[inline(always)]
pub fn fork_return(trap_context: &mut TrapContext) -> ! {
    extern "C" {
        fn __fork_return(a0: usize) -> !;
    }
    before_trap_return();
    unsafe { __fork_return(trap_context as *mut TrapContext as usize) }
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
