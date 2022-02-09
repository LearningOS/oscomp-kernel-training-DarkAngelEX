use core::arch::global_asm;

use crate::riscv::{
    cpu,
    register::{
        scause, sie,
        stvec::{self, TrapMode},
    },
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

/// return value is sscratch = ptr of TrapContext
#[no_mangle]
pub fn trap_handler() -> usize {
    todo!("trap_handler todo!");
    before_trap_return();
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
pub fn trap_return(trap_context: &TrapContext) -> ! {
    extern "C" {
        fn __trap_return(a0: usize) -> !;
    }
    before_trap_return();
    unsafe { __trap_return(trap_context as *const TrapContext as usize) }
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
