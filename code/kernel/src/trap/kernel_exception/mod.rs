mod page_fault;

use riscv::register::{scause, sepc, sstatus, stval};

use crate::{
    hart::{self, cpu},
    local,
    xdebug::trace,
};

#[no_mangle]
pub fn kernel_default_exception() {
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
    
    let exception = match scause::read().cause() {
        scause::Trap::Exception(e) => e,
        scause::Trap::Interrupt(i) => panic!("should kernel_exception but {:?}", i),
    };
    match exception {
        scause::Exception::InstructionMisaligned => todo!(),
        scause::Exception::InstructionFault => todo!(),
        scause::Exception::IllegalInstruction => todo!(),
        scause::Exception::Breakpoint => todo!(),
        scause::Exception::LoadFault => todo!(),
        scause::Exception::StoreMisaligned => todo!(),
        scause::Exception::StoreFault => todo!(),
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
    return;
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
