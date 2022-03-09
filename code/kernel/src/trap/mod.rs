use core::arch::{asm, global_asm};
use core::convert::TryFrom;

use riscv::register::{sepc, sstatus};

use crate::hart::{self, cpu};
use crate::memory::address::UserAddr;
use crate::riscv::register::{
    scause, sie, stval,
    stvec::{self, TrapMode},
};
use crate::{local, timer, tools};

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

// 如果发生了用户态访存错误，直接填0
#[no_mangle]
pub fn trap_from_kernel() {
    unsafe { close_kernel_trap_entry() };
    stack_trace!();
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
            e @ (scause::Exception::LoadPageFault | scause::Exception::StorePageFault) => {
                let mut error = true;
                stack_trace!();
                let local = local::task_local();
                if local.sum_cur() != 0 {
                    assert!(local.user_access_status.not_forbid());
                    let stval = get_stval();
                    if let Ok(addr) = UserAddr::try_from(stval as *const u8) {
                        if local.user_access_status.is_access() {
                            println!("access user data error! ignore this instruction. {:?} stval: {:#x}", e, stval);
                            local.user_access_status.set_error(addr, e);
                        }
                        sepc::write(tools::next_sepc(get_sepc()));
                        error = false;
                    }
                } else {
                    assert!(local.user_access_status.is_forbid());
                }
                if error {
                    fatal_error()
                }
            }
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
    fn get_stval() -> usize {
        stval::read()
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

#[inline(always)]
pub unsafe fn set_kernel_trap_entry() {
    extern "C" {
        fn __kernel_trap_entry();
    }
    stvec::write(__kernel_trap_entry as usize, TrapMode::Direct);
}

#[inline(always)]
pub unsafe fn close_kernel_trap_entry() {
    stvec::write(loop_forever as usize, TrapMode::Direct);
}

#[inline(always)]
unsafe fn set_user_trap_entry() {
    stvec::write(get_return_from_user(), TrapMode::Direct);
}

#[inline(always)]
pub fn enable_timer_interrupt() {
    unsafe {
        sie::set_stimer();
    }
}
