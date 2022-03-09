use core::arch::global_asm;

use riscv::register::sstatus;

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

#[inline(always)]
pub unsafe fn set_kernel_default_trap() {
    extern "C" {
        fn __kernel_default_vector();
    }
    stvec::write(__kernel_default_vector as usize, TrapMode::Vectored);
}

// #[inline(always)]
// pub unsafe fn close_kernel_trap() {
//     fn loop_forever() -> ! {
//         println!("entry loop_forever");
//         loop {}
//     }
//     stvec::write(loop_forever as usize, TrapMode::Direct);
// }

#[inline(always)]
unsafe fn set_user_trap_entry() {
    extern "C" {
        fn __return_from_user();
    }
    debug_check!(!sstatus::read().sie());
    stvec::write(__return_from_user as usize, TrapMode::Direct);
}

#[inline(always)]
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}
