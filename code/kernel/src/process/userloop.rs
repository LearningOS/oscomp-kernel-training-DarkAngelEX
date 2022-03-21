use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloc::{boxed::Box, sync::Arc};
use riscv::register::{
    scause::{self, Exception, Interrupt},
    stval,
};

use crate::{
    executor,
    local::{self, always_local::AlwaysLocal, task_local::TaskLocal, LocalNow},
    process::thread,
    syscall::Syscall,
    timer,
    user::AutoSie,
};

use super::thread::Thread;

async fn userloop(thread: Arc<Thread>) {
    stack_trace!(to_yellow!("running in user loop"));
    loop {
        local::handle_current_local();

        let mut do_exit = false;

        let context = thread.as_ref().get_context();
        let auto_sie = AutoSie::new();
        // let mut do_yield = false;
        match thread.process.alive_then(|_a| ()) {
            Ok(_x) => context.run_user(),
            Err(_e) => do_exit = true,
        };
        if !do_exit {
            let scause = scause::read().cause();
            let stval = stval::read();

            drop(auto_sie);

            let mut user_fatal_error = || {
                println!(
                    "[kernel]user_fatal_error {:?} {:?} {:?} stval: {:#x}",
                    thread.process.pid(),
                    thread.tid,
                    scause,
                    stval,
                );
                do_exit = true;
            };

            match scause {
                scause::Trap::Exception(e) => match e {
                    Exception::UserEnvCall => {
                        // println!("enter syscall");
                        do_exit = Syscall::new(context, &thread, &thread.process)
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
                        timer::tick();
                        if !do_exit {
                            thread::yield_now().await;
                        }
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
    let future = userloop(thread.clone());
    // let (runnable, task) = executor::spawn(future);
    let (runnable, task) = executor::spawn(OutermostFuture::new(thread, future));
    runnable.schedule();
    task.detach();
}

struct OutermostFuture<F: Future + Send + 'static> {
    future: Pin<Box<F>>,
    local_switch: LocalNow,
}
impl<F: Future + Send + 'static> OutermostFuture<F> {
    pub fn new(thread: Arc<Thread>, future: F) -> Self {
        let page_table = thread
            .process
            .alive_then(|a| a.user_space.page_table_arc())
            .unwrap();
        let local_switch = LocalNow::Task(Box::new(TaskLocal {
            always_local: AlwaysLocal::new(),
            thread,
            page_table,
        }));
        Self {
            future: Box::pin(future),
            local_switch,
        }
    }
}

impl<F: Future + Send + 'static> Future for OutermostFuture<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let local = local::hart_local();
        local.enter_task_switch(&mut self.local_switch);
        let ret = self.future.as_mut().poll(cx);
        local.leave_task_switch(&mut self.local_switch);
        ret
    }
}
