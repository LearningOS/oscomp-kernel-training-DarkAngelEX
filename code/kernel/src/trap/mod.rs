use core::arch::global_asm;

use riscv::register::{scause, sstatus};

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
    test_interrupt();
}

pub fn test_interrupt() {
    println!("[FTL OS]trap init");
    let sie = sstatus::read().sie();
    unsafe { sstatus::set_sie() };
    // 给自己发个中断!!!

    if !sie {
        unsafe { sstatus::clear_sie() };
    }
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
pub fn kernel_default_trap() {
    stack_trace!();
    match scause::read().cause() {
        scause::Trap::Interrupt(_) => kernel_interrupt::kernel_default_interrupt(),
        scause::Trap::Exception(_) => kernel_exception::kernel_default_exception(),
    }
}

#[inline(always)]
pub unsafe fn set_kernel_default_trap() {
    extern "C" {
        fn __kernel_default_vector();
        fn __kernel_default_trap_entry();
    }
    if true {
        stvec::write(__kernel_default_vector as usize, TrapMode::Vectored);
    } else {
        stvec::write(__kernel_default_trap_entry as usize, TrapMode::Direct);
    }
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
