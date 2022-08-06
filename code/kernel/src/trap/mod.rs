use crate::riscv::register::{
    scause, sie, sstatus,
    stvec::{self, TrapMode},
};

use self::context::UKContext;

pub mod context;
mod kernel_exception;
mod kernel_interrupt;

core::arch::global_asm!(include_str!("trap.S"));

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

/// 由执行器调用, 进入用户态, 并在原地返回
#[inline(always)]
pub fn run_user_executor(cx: &mut UKContext) {
    extern "C" {
        // 返回值: fast_processing_path 返回的a1
        fn __entry_user(cx: *mut UKContext) -> usize;
    }
    unsafe {
        debug_assert!(sstatus::read().sie());
        sstatus::clear_sie();
        set_user_trap_entry();
        let _s = __entry_user(cx);
        // fast_processing_path 中已经恢复了内核态环境
        debug_assert!(sstatus::read().sie());
        // set_kernel_default_trap();
    }
}

/// 这两个变量会放入a0和a1
#[repr(C)]
pub struct Ctup2(pub *mut UKContext, pub usize);
/// 内核态同步快速处理路径
///
/// return:
///
///     (_, 0): 进入用户态
///     (_, _): 回到executor
///
/// 进入__entry_user之后一定会执行一次, 因此需要在这里恢复内核态环境
#[no_mangle]
pub unsafe extern "C" fn fast_processing_path(cx: *mut UKContext) -> Ctup2 {
    set_kernel_default_trap();
    sstatus::set_sie();
    let to_executor = 1;
    if to_executor != 0 {
    } else {
        sstatus::clear_sie();
        set_user_trap_entry();
    }
    Ctup2(cx, to_executor)
}

/// 内核态陷阱
#[no_mangle]
pub fn kernel_default_trap(a0: usize) {
    stack_trace!();
    match scause::read().cause() {
        scause::Trap::Interrupt(_) => kernel_interrupt::kernel_default_interrupt(),
        scause::Trap::Exception(_) => kernel_exception::kernel_default_exception(a0),
    }
}

#[inline(always)]
pub unsafe fn set_kernel_default_trap() {
    extern "C" {
        fn __kernel_default_trap_vector();
        fn __kernel_default_trap_entry();
    }
    if true {
        stvec::write(__kernel_default_trap_vector as usize, TrapMode::Vectored);
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
