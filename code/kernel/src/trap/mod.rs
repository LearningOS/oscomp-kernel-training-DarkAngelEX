use core::arch::global_asm;

use riscv::register::{
    scause::{self, Trap},
    sstatus,
};

use crate::riscv::register::{
    sie,
    stvec::{self, TrapMode},
};

use self::context::UKContext;

pub mod context;
mod kernel_exception;
mod kernel_interrupt;

global_asm!(include_str!("trap.S"));

pub fn init() {
    println!("[FTL OS]trap init");
    unsafe { set_kernel_default_trap() };
}

#[inline(always)]
pub fn run_user(cx: &mut UKContext) {
    extern "C" {
        fn __entry_user(cx: *mut UKContext);
    }
    unsafe {
        set_user_trap_entry();
        __entry_user(cx);
        set_kernel_default_trap();
    };
}

#[no_mangle]
pub extern "C" fn kernel_default_trap() {
    stack_trace!();
    match scause::read().cause() {
        Trap::Exception(e) => kernel_exception::kernel_default_exception(e),
        Trap::Interrupt(i) => kernel_interrupt::kernel_default_interrupt(i),
    };
}

#[inline(always)]
pub unsafe fn set_kernel_default_trap() {
    extern "C" {
        fn __kernel_default_trap();
    }
    stvec::write(__kernel_default_trap as usize, TrapMode::Direct);
}

#[inline(always)]
unsafe fn set_user_trap_entry() {
    extern "C" {
        fn __return_from_user();
    }
    debug_assert!(!sstatus::read().sie());
    stvec::write(__return_from_user as usize, TrapMode::Direct);
}

#[inline(always)]
pub fn enable_timer_interrupt() {
    unsafe { sie::set_stimer() };
}
