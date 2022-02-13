use core::arch::{asm, global_asm};

// ASCII: Goto Task
#[allow(unused)]
pub const ADD_TASK_MAGIC: usize = 0x476F746F_5461736B;

use crate::{
    debug::{PRINT_FORK, PRINT_TRAP, PRINT_SPECIAL_RETURN},
    riscv::{
        cpu,
        register::{
            scause::{self, Exception, Interrupt},
            sie, stval,
            stvec::{self, TrapMode},
        },
    },
    scheduler::{self, get_current_task, get_current_task_ptr},
    syscall, timer,
    user::AutoSum,
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
    memory_trace!("syscall_handler entry");
    let tcb_ptr = trap_context.tcb.get_ref() as *const _;
    debug_check_ne!(tcb_ptr, core::ptr::null());
    debug_check_eq!(tcb_ptr, get_current_task_ptr());
    debug_check_eq!(
        trap_context.kernel_stack,
        trap_context.get_tcb().kernel_bottom()
    );
    assert!(trap_context.need_add_task == 0);
    if PRINT_TRAP {
        println!(
            "call syscall_handler hart: {} {:?} a7:{}",
            cpu::hart_id(),
            trap_context.get_tcb().pid(),
            a7
        );
    }
    trap_context.into_next_instruction();
    let ret = syscall::syscall(trap_context, a7, [a0, a1, a2]);
    if trap_context.need_add_task == 0 {
        before_trap_return();
    } else {
        debug_check!(trap_context.need_add_task == ADD_TASK_MAGIC);
    }
    memory_trace!("syscall_handler return");
    (ret, trap_context)
}
/// return value is sscratch = ptr of TrapContext
#[no_mangle]
pub extern "C" fn trap_handler(trap_context: &mut TrapContext) -> &mut TrapContext {
    set_kernel_trap_entry();
    let tcb_ptr = trap_context.tcb.get_ref() as *const _;
    debug_check_ne!(tcb_ptr, core::ptr::null());
    debug_check_eq!(tcb_ptr, get_current_task_ptr());
    debug_check_eq!(
        trap_context.kernel_stack,
        trap_context.get_tcb().kernel_bottom()
    );
    println!("enter trap_handler");
    let scause = scause::read();
    let stval = stval::read();
    match scause.cause() {
        scause::Trap::Exception(e) => match e {
            Exception::UserEnvCall => {
                panic!("should into syscall_handler!");
            }
            Exception::StoreFault
            | Exception::StorePageFault
            | Exception::LoadFault
            | Exception::LoadPageFault
            | Exception::InstructionFault
            | Exception::InstructionPageFault => {
                let x = AutoSum::new();
                println!(
                "[kernel] {:?} in application {:?} kernel killed it. bad addr = {:#x}, bad instruction = {:?}, user_sp: {:#x} hart: {}",
                scause.cause(),
                get_current_task().pid(),
                stval,
                trap_context.sepc(),
                trap_context.x[2],
                cpu::hart_id()
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
            Interrupt::SupervisorTimer => {
                timer::set_next_trigger();
                scheduler::app::suspend_current_and_run_next(trap_context);
            }
            Interrupt::UserExternal => todo!(),
            Interrupt::VirtualSupervisorExternal => todo!(),
            Interrupt::SupervisorExternal => todo!(),
            Interrupt::Unknown => todo!(),
        },
    }
    memory_trace!("trap_handler return");
    before_trap_return();
    trap_context
}

#[no_mangle]
#[allow(improper_ctypes_definitions)]
pub extern "C" fn trap_after_save_sx(
    a0: usize,
    trap_context: &mut TrapContext,
) -> (isize, *mut TrapContext) {
    // now only fork run this.
    memory_trace!("trap_after_save_sx entry");
    assert_eq!(trap_context.need_add_task, ADD_TASK_MAGIC);
    trap_context.need_add_task = 0;
    syscall::assert_fork(a0);
    let task_new = trap_context.task_new.take().unwrap();
    let pid = task_new.pid().into_usize();
    task_new.as_ref().set_user_ret(0);
    scheduler::add_task(task_new);
    // println!("fork return: {}", pid);
    memory_trace!("trap_after_save_sx return");
    if PRINT_FORK || PRINT_SPECIAL_RETURN {
        println!(
            "!!! fork_old_return {:?} -> {} !!! hart = {} sepc: {:#x}",
            trap_context.get_tcb().pid(),
            pid as isize,
            cpu::hart_id(),
            trap_context.sepc().into_usize()
        );
    }
    // scheduler::app::suspend_current_and_run_next(trap_context); // let subpross run first
    before_trap_return();
    (pid as isize, trap_context)
}

#[no_mangle]
pub fn trap_from_kernel() -> ! {
    memory_trace_show!("trap_from_kernel entry");
    let sepc: usize;
    unsafe {
        asm!("csrr {}, sepc", out(reg)sepc);
    }
    panic!(
        "a trap {:?} from kernel! bad addr = {:#x}, sepc = {:#x}, hart = {}",
        scause::read().cause(),
        stval::read(),
        sepc,
        cpu::hart_id()
    );
}

#[inline(always)]
pub fn before_trap_return() {
    if PRINT_TRAP {
        println!(
            "call before_trap_return hart: {} {:?}",
            cpu::hart_id(),
            get_current_task().pid()
        );
    }
    set_user_trap_entry();
}

pub fn exec_return(trap_context: &mut TrapContext) -> ! {
    extern "C" {
        fn __exec_return(a0: usize) -> !;
    }
    if PRINT_SPECIAL_RETURN {
        println!(
            "!!! exec_return {:?} !!! hart = {} trap_ptr: {:#x} sp: {:#x}",
            trap_context.get_tcb().pid(),
            cpu::hart_id(),
            trap_context as *const _ as usize,
            trap_context.x[2]
        );
    }
    before_trap_return();
    unsafe { __exec_return(trap_context as *mut TrapContext as usize) }
}

pub fn fork_return(trap_context: &mut TrapContext) -> ! {
    extern "C" {
        fn __fork_return(a0: usize) -> !;
    }
    if PRINT_FORK || PRINT_SPECIAL_RETURN {
        println!(
            "!!! fork_new_return {:?} -> {} !!! hart = {}",
            trap_context.get_tcb().pid(),
            trap_context.x[10],
            cpu::hart_id()
        );
    }
    before_trap_return();
    unsafe { __fork_return(trap_context as *mut TrapContext as usize) }
}

pub fn set_kernel_trap_entry() {
    unsafe {
        stvec::write(trap_from_kernel as usize, TrapMode::Direct);
    }
}

#[inline(always)]
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
