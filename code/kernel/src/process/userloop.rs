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
    hart::sfence,
    local::{self, always_local::AlwaysLocal, task_local::TaskLocal, LocalNow},
    process::{thread, Pid},
    syscall::Syscall,
    timer,
    tools::allocator::from_usize_allocator::FromUsize,
    user::{trap_handler, AutoSie},
};

use super::thread::Thread;

async fn userloop(thread: Arc<Thread>) {
    stack_trace!(to_yellow!("running in user loop"));
    loop {
        local::handle_current_local();

        let context = thread.as_ref().get_context();
        let auto_sie = AutoSie::new();
        // let mut do_yield = false;
        if false {
            // debug
            sfence::sfence_vma_all_global();
        }
        match thread.process.alive_then(|_a| ()) {
            Ok(_x) => context.run_user(),
            Err(_e) => break,
        };

        let scause = scause::read().cause();
        let stval = stval::read();

        drop(auto_sie);

        let mut do_exit = false;
        let mut user_fatal_error = || {
            println!(
                "[kernel]user_fatal_error {:?} {:?} {:?} stval: {:#x} sepc: {:#x}",
                thread.process.pid(),
                thread.tid,
                scause,
                stval,
                context.user_sepc
            );
            do_exit = true;
        };

        match scause {
            scause::Trap::Exception(e) => match e {
                Exception::UserEnvCall => {
                    // println!("enter syscall {}", context.a7());
                    do_exit = Syscall::new(context, &thread, &thread.process)
                        .syscall()
                        .await;
                }
                e @ (Exception::InstructionPageFault
                | Exception::LoadPageFault
                | Exception::StorePageFault) => {
                    do_exit = trap_handler::page_fault(&thread, e, stval, context.user_sepc).await;
                }
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
        if do_exit {
            break;
        }
        // if do_yield {
        //     thread::yield_now().await;
        // }
    }
    if thread.process.pid() == Pid::from_usize(0) {
        panic!("initproc exit");
    }
    if let Some(alive) = &mut *thread.process.alive.lock(place!()) {
        // TODO: just last thread exit do this.
        println!("[kernel]proc:{:?} abort", thread.process.pid());
        alive.clear_all(thread.process.pid());
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
