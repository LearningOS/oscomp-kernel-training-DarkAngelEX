use core::arch::{asm, global_asm};

use crate::hart::{self, cpu};
use crate::riscv::register::{
    scause, sie, stval,
    stvec::{self, TrapMode},
};

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
pub fn trap_from_kernel() -> ! {
    unsafe { close_kernel_trap_entry() };
    memory_trace_show!("trap_from_kernel entry");
    let sepc: usize;
    unsafe {
        asm!("csrr {}, sepc", out(reg)sepc);
    }
    panic!(
        "a trap {:?} from kernel! bad addr = {:#x}, sepc = {:#x}, hart = {} sp = {:#x}",
        scause::read().cause(),
        stval::read(),
        sepc,
        cpu::hart_id(),
        hart::current_sp()
    );
}
#[no_mangle]
pub fn loop_forever() -> ! {
    println!("entry loop_forever");
    loop {}
}

pub unsafe fn set_kernel_trap_entry() {
    stvec::write(trap_from_kernel as usize, TrapMode::Direct);
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
