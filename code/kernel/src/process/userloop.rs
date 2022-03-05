use alloc::{boxed::Box, sync::Arc};
use riscv::register::scause::{self, Exception, Interrupt};

use crate::{executor, local, syscall::Syscall, timer, xdebug::stack_trace::StackTrace};

use super::thread::Thread;

async fn userloop(thread: Arc<Thread>) {
    loop {
        let mut stack_trace = Box::pin(StackTrace::new());

        local::handle_current_local();
        let context = thread.as_ref().get_context();

        let mut do_exit = false;
        // let mut do_yield = false;
        match thread.process.using_space() {
            Ok(guard) => context.run_user(&guard),
            Err(_e) => do_exit = true,
        };
        if !do_exit {
            let mut user_fatal_error = || {
                println!(
                    "[kernel]{:?} {:?} exception: {:?}",
                    thread.process.pid(),
                    thread.tid,
                    scause::read().cause()
                );
                do_exit = true;
            };
            match scause::read().cause() {
                scause::Trap::Exception(e) => match e {
                    Exception::UserEnvCall => {
                        // println!("enter syscall");
                        do_exit =
                            Syscall::new(context, &thread, &thread.process, stack_trace.as_mut())
                                .syscall()
                                .await;
                    }
                    Exception::InstructionPageFault
                    | Exception::LoadPageFault
                    | Exception::StorePageFault => user_fatal_error(),
                    Exception::InstructionMisaligned => todo!(),
                    Exception::InstructionFault => user_fatal_error(),
                    Exception::IllegalInstruction => user_fatal_error(),
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
        }
        if do_exit {
            let mut lock = thread.process.alive.lock(place!());
            if let Some(alive) = &mut *lock {
                // TODO: just last thread exit do this.
                println!("[kernel]proc:{:?} abort", thread.process.pid());
                alive.clear_all(thread.process.pid());
            }
            break;
        }
        // if do_yield {
        //     thread::yield_now().await;
        // }
    }
}

pub fn spawn(thread: Arc<Thread>) {
    let future = userloop(thread);
    let (runnable, task) = executor::spawn(future);
    runnable.schedule();
    task.detach();
}
