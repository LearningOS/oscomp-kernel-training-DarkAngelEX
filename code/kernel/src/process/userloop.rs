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
    memory::asid::USING_ASID,
    process::{exit, thread, Dead, Pid},
    syscall::Syscall,
    timer,
    user::{trap_handler, AutoSie},
    xdebug::PRINT_SYSCALL_ALL,
};

use super::thread::Thread;

async fn userloop(thread: Arc<Thread>) {
    stack_trace!(to_yellow!("running in user loop"));
    if PRINT_SYSCALL_ALL {
        println!("{}", to_yellow!("<new thread into userloop>"));
    }
    if thread.process.is_alive() {
        thread.settid().await;
    }
    let context = thread.get_context();
    loop {
        local::handle_current_local();

        let auto_sie = AutoSie::new();

        // sfence::sfence_vma_all_global();

        if !thread.process.is_alive() {
            break;
        }
        if let Err(Dead) = thread.handle_signal().await {
            break;
        }

        {
            let local = local::hart_local();
            local.local_rcu.critical_end_tick();
            local.local_rcu.critical_start();
            thread.timer_into_user();
            context.run_user();
            thread.timer_leave_user();
            local.local_rcu.critical_start();
        }

        let scause = scause::read().cause();
        let stval = stval::read();

        drop(auto_sie);

        local::handle_current_local();

        let mut do_exit = false;
        let mut user_fatal_error = || {
            println!(
                "[kernel]user_fatal_error userloop {:?} {:?} {:?} stval: {:#x} sepc: {:#x} ra: {:#x}",
                thread.process.pid(),
                thread.tid(),
                scause,
                stval,
                context.user_sepc,
                context.ra(),
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
                        // println!("yield by timer: {:?}", thread.tid());
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
    }
    if thread.process.pid() == Pid(0) {
        panic!("initproc exit");
    }
    exit::exit_impl(&thread).await;
}

pub fn spawn(thread: Arc<Thread>) {
    let future = OutermostFuture::new(thread.clone(), userloop(thread));
    let (runnable, task) = executor::spawn(future);
    runnable.schedule();
    task.detach();
}

struct OutermostFuture<F: Future + Send + 'static> {
    future: F,
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
            future,
            local_switch,
        }
    }
}

impl<F: Future + Send + 'static> Future for OutermostFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let local = local::hart_local();
        local.handle();
        local.local_rcu.critical_start();
        let this = unsafe { self.get_unchecked_mut() };
        local.enter_task_switch(&mut this.local_switch);
        if !USING_ASID {
            sfence::sfence_vma_all_no_global();
        }
        let ret = unsafe { Pin::new_unchecked(&mut this.future).poll(cx) };
        local.leave_task_switch(&mut this.local_switch);
        if !USING_ASID {
            sfence::sfence_vma_all_no_global();
        }
        local.local_rcu.critical_end_tick();
        ret
    }
}
