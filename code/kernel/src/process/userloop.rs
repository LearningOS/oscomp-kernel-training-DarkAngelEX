use alloc::sync::Arc;
use riscv::register::scause;

use crate::{executor, syscall::Syscall};

use super::thread::Thread;

async fn userloop(thread: Arc<Thread>) {
    loop {
        let context = thread.as_ref().get_context();
        thread.process.using_space(); // !!!
                                      // println!("enter user {:?}", thread.process.pid());
        context.run_user();
        // println!("return from user");
        let mut do_exit = false;
        let mut do_yield = false;
        match scause::read().cause() {
            scause::Trap::Exception(e) => match e {
                scause::Exception::InstructionMisaligned => todo!(),
                scause::Exception::InstructionFault => todo!(),
                scause::Exception::IllegalInstruction => todo!(),
                scause::Exception::Breakpoint => todo!(),
                scause::Exception::LoadFault => todo!(),
                scause::Exception::StoreMisaligned => todo!(),
                scause::Exception::StoreFault => todo!(),
                scause::Exception::UserEnvCall => {
                    // println!("enter syscall");
                    Syscall::new(
                        context,
                        thread.as_ref(),
                        thread.process.as_ref(),
                        &mut do_exit,
                        &mut do_yield,
                    )
                    .syscall()
                    .await;
                }
                scause::Exception::VirtualSupervisorEnvCall => todo!(),
                scause::Exception::InstructionPageFault => todo!(),
                scause::Exception::LoadPageFault => todo!(),
                scause::Exception::StorePageFault => todo!(),
                scause::Exception::InstructionGuestPageFault => todo!(),
                scause::Exception::LoadGuestPageFault => todo!(),
                scause::Exception::VirtualInstruction => todo!(),
                scause::Exception::StoreGuestPageFault => todo!(),
                scause::Exception::Unknown => todo!(),
            },
            scause::Trap::Interrupt(i) => match i {
                scause::Interrupt::UserSoft => todo!(),
                scause::Interrupt::VirtualSupervisorSoft => todo!(),
                scause::Interrupt::SupervisorSoft => todo!(),
                scause::Interrupt::UserTimer => todo!(),
                scause::Interrupt::VirtualSupervisorTimer => todo!(),
                scause::Interrupt::SupervisorTimer => {
                    todo!();
                }
                scause::Interrupt::UserExternal => todo!(),
                scause::Interrupt::VirtualSupervisorExternal => todo!(),
                scause::Interrupt::SupervisorExternal => todo!(),
                scause::Interrupt::Unknown => todo!(),
            },
        }
        if do_exit {
            todo!()
        }
        if do_yield {
            todo!()
        }
    }
}

pub fn spawn(thread: Arc<Thread>) {
    let future = userloop(thread);
    let (runnable, task) = executor::spawn(future);
    runnable.schedule();
    task.detach();
}
