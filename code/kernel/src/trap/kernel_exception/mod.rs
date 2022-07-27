mod page_fault;

use riscv::register::{
    scause::{self, Exception, Trap},
    sepc, sstatus, stval,
};

use crate::{
    hart::{self, cpu},
    local, tools,
    xdebug::trace,
};

#[no_mangle]
pub fn kernel_default_exception(a0: usize) {
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
        Trap::Exception(e) => e,
        Trap::Interrupt(i) => panic!("should kernel_exception but {:?}", i),
    };
    match exception {
        Exception::InstructionMisaligned => todo!(),
        Exception::InstructionFault => {
            println!("InstructionFault ra: {} sepc: {}", KTrapCX::new_ref(a0).ra(), sepc);
            fatal_exception_error();
        }
        Exception::IllegalInstruction => {
            println!("illiegal IR of sepc: {:#x}", sepc);
            todo!();
        }
        Exception::Breakpoint => {
            println!("breakpoint of sepc: {:#x}", sepc);
            sepc = tools::next_sepc(sepc);
        }
        Exception::LoadFault => fatal_exception_error(),
        Exception::StoreMisaligned => fatal_exception_error(),
        Exception::StoreFault => fatal_exception_error(),
        Exception::UserEnvCall => todo!(),
        Exception::VirtualSupervisorEnvCall => todo!(),
        Exception::InstructionPageFault => fatal_exception_error(),
        e @ (Exception::LoadPageFault | Exception::StorePageFault) => {
            sepc = page_fault::page_fault_handle(e, stval, sepc);
        }
        Exception::InstructionGuestPageFault => todo!(),
        Exception::LoadGuestPageFault => todo!(),
        Exception::VirtualInstruction => todo!(),
        Exception::StoreGuestPageFault => todo!(),
        Exception::Unknown => fatal_exception_error(),
    }

    *in_exception = false;
    sepc::write(sepc);
}

fn fatal_exception_error() -> ! {
    let sepc = sepc::read();
    println!(
        "kernel fatal_exception_error! {:?} bad addr = {:#x}, sepc = {:#x}, hart = {} sp = {:#x}",
        scause::read().cause(),
        stval::read(),
        sepc,
        cpu::hart_id(),
        hart::current_sp(),
    );
    panic!()
}

#[repr(C)]
struct KTrapCX {
    _unused: usize,
    ra: usize,
    t0: usize,
    t1: usize,
    t2: usize,
    t3: usize,
    t4: usize,
    t5: usize,
    t6: usize,
    a0: usize,
    a1: usize,
    a2: usize,
    a3: usize,
    a4: usize,
    a5: usize,
    a6: usize,
    a7: usize,
}

impl KTrapCX {
    pub fn new_ref(a0: usize) -> &'static Self {
        unsafe { core::mem::transmute(a0) }
    }
    pub fn ra(&self) -> usize {
        self.ra
    }
}
