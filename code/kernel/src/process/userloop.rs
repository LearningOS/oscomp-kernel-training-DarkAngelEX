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
    local::{self, task_local::TaskLocal},
    process::thread,
    syscall::Syscall,
    timer,
    user::{AutoSie, UserAccessStatus},
    xdebug::stack_trace::StackTrace,
};

use super::thread::Thread;

async fn userloop(thread: Arc<Thread>) {
    loop {
        stack_trace!();
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
            let mut user_fatal_error = || {
                println!(
                    "[kernel]user_fatal_error {:?} {:?} {:?} stval: {:#x}",
                    thread.process.pid(),
                    thread.tid,
                    scause::read().cause(),
                    stval::read(),
                );
                do_exit = true;
            };
            match scause::read().cause() {
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
        drop(auto_sie);
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
    task: Option<Box<TaskLocal>>,
}
impl<F: Future + Send + 'static> OutermostFuture<F> {
    pub fn new(thread: Arc<Thread>, future: F) -> Self {
        let page_table = thread
            .process
            .alive_then(|a| a.user_space.page_table_arc())
            .unwrap();
        let task = Box::new(TaskLocal {
            thread,
            page_table,
            sie_count: 0,
            sum_count: 0,
            user_access_status: UserAccessStatus::Forbid,
            stack_trace: StackTrace::new(),
        });
        Self {
            future: Box::pin(future),
            task: Some(task),
        }
    }
}

impl<F: Future + Send + 'static> Future for OutermostFuture<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let local = local::hart_local();
        local.set_task(self.task.take().unwrap());

        let ret = self.future.as_mut().poll(cx);

        self.task = Some(local.take_task());
        ret
    }
}
