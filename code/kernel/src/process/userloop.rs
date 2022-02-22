use alloc::{boxed::Box, sync::Arc};
use riscv::register::scause::{self, Exception, Interrupt};

use crate::{executor, local, syscall::Syscall, timer, xdebug::stack_trace::StackTrace};

use super::thread::{self, Thread};

async fn userloop(thread: Arc<Thread>) {
    loop {
        let mut stack_trace = Box::pin(StackTrace::new());

        local::handle_current_local();
        let context = thread.as_ref().get_context();

        {
            // println!("enter user {:?}", thread.process.pid());
            let _guard = thread.process.using_space();
            context.run_user();
            // println!("return from user");
        }
        let mut do_exit = false;
        let mut do_yield = false;
        match scause::read().cause() {
            scause::Trap::Exception(e) => match e {
                Exception::UserEnvCall => {
                    // println!("enter syscall");
                    do_exit = Syscall::new(
                        context,
                        &thread,
                        thread.process.as_ref(),
                        stack_trace.as_mut(),
                    )
                    .syscall()
                    .await;
                }
                Exception::InstructionPageFault
                | Exception::LoadPageFault
                | Exception::StorePageFault => todo!(),
                Exception::InstructionMisaligned => todo!(),
                Exception::InstructionFault => todo!(),
                Exception::IllegalInstruction => todo!(),
                Exception::Breakpoint => todo!(),
                Exception::LoadFault => todo!(),
                Exception::StoreMisaligned => todo!(),
                Exception::StoreFault => todo!(),
                Exception::VirtualSupervisorEnvCall => todo!(),
                Exception::InstructionGuestPageFault => todo!(),
                Exception::LoadGuestPageFault => todo!(),
                Exception::VirtualInstruction => todo!(),
                Exception::StoreGuestPageFault => todo!(),
                Exception::Unknown => todo!(),
            },
            scause::Trap::Interrupt(i) => match i {
                Interrupt::UserSoft => todo!(),
                Interrupt::VirtualSupervisorSoft => todo!(),
                Interrupt::SupervisorSoft => todo!(),
                Interrupt::UserTimer => todo!(),
                Interrupt::VirtualSupervisorTimer => todo!(),
                Interrupt::SupervisorTimer => {
                    // do_yield = true;
                    timer::tick();
                }
                Interrupt::UserExternal => todo!(),
                Interrupt::VirtualSupervisorExternal => todo!(),
                Interrupt::SupervisorExternal => todo!(),
                Interrupt::Unknown => todo!(),
            },
        }
        if do_exit {
            break;
        }
        if do_yield {
            thread::yield_now().await;
        }
    }
}

pub fn spawn(thread: Arc<Thread>) {
    let future = userloop(thread);
    let (runnable, task) = executor::spawn(future);
    runnable.schedule();
    task.detach();
}
