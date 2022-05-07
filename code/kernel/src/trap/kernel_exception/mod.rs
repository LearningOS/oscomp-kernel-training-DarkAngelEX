mod page_fault;

use riscv::register::{scause::{self, Exception}, sepc, sstatus, stval};

use crate::{
    hart::{self, cpu},
    local, tools,
    xdebug::trace,
};


pub fn kernel_default_exception(exception: Exception) {
    stack_trace!();
    trace::stack_detection();
    // 中断已经被关闭
    assert!(!sstatus::read().sie());
    // 禁止异常处理嵌套
    let in_exception = &mut local::hart_local().in_exception;
    assert!(!*in_exception);
    *in_exception = true;
    let mut sepc = sepc::read();
    let stval = stval::read();

    match exception {
        scause::Exception::InstructionMisaligned => todo!(),
        scause::Exception::InstructionFault => todo!(),
        scause::Exception::IllegalInstruction => {
            println!("illiegal IR of sepc: {:#x}", sepc);
            todo!();
        }
        scause::Exception::Breakpoint => {
            println!("breakpoint of sepc: {:#x}", sepc);
            sepc = tools::next_sepc(sepc);
        }
        scause::Exception::LoadFault => fatal_exception_error(),
        scause::Exception::StoreMisaligned => fatal_exception_error(),
        scause::Exception::StoreFault => fatal_exception_error(),
        scause::Exception::UserEnvCall => todo!(),
        scause::Exception::VirtualSupervisorEnvCall => todo!(),
        scause::Exception::InstructionPageFault => fatal_exception_error(),
        e @ (scause::Exception::LoadPageFault | scause::Exception::StorePageFault) => {
            sepc = page_fault::page_fault_handle(e, stval, sepc);
        }
        scause::Exception::InstructionGuestPageFault => todo!(),
        scause::Exception::LoadGuestPageFault => todo!(),
        scause::Exception::VirtualInstruction => todo!(),
        scause::Exception::StoreGuestPageFault => todo!(),
        scause::Exception::Unknown => fatal_exception_error(),
    }

    *in_exception = false;
    sepc::write(sepc);
}

fn fatal_exception_error() -> ! {
    let sepc = sepc::read();
    panic!(
        "kernel fatal_exception_error! {:?} bad addr = {:#x}, sepc = {:#x}, hart = {} sp = {:#x}",
        scause::read().cause(),
        stval::read(),
        sepc,
        cpu::hart_id(),
        hart::current_sp(),
    );
}
