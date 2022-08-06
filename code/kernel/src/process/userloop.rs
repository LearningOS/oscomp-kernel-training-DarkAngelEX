use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloc::{boxed::Box, sync::Arc};
use riscv::register::scause::{self, Exception, Interrupt};

use crate::{
    executor,
    hart::sfence,
    local::{self, always_local::AlwaysLocal, task_local::TaskLocal, LocalNow},
    memory::asid::USING_ASID,
    process::{exit, thread, Dead, Pid},
    syscall::Syscall,
    timer,
    trap::context::FastContext,
    user::trap_handler,
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
    let fast_context = unsafe { FastContext::new(&thread, &thread, &thread.process) };
    context.set_fast_context(&fast_context);

    loop {
        local::handle_current_local();

        // sfence::sfence_vma_all_global();
        debug_assert!(thread.process.is_alive());

        if let Err(Dead) = thread.handle_signal().await {
            break;
        }
        {
            let local = local::hart_local();
            local.local_rcu.critical_end_tick();
            local.local_rcu.critical_start();
            thread.timer_into_user();
            context.run_user_executor();
            thread.timer_leave_user();
            local.local_rcu.critical_start();
        }

        local::handle_current_local();
        let scause = context.scause.cause();
        let stval = context.stval;

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
                    thread.timer_fence();
                    {
                        use crate::signal::*;
                        let mut timer = thread.process.timer.lock();
                        if timer.suspend_real() {
                            thread.receive(Sig::from_user(SIGALRM as u32).unwrap());
                        }
                        if timer.suspend_virtual() {
                            thread.receive(Sig::from_user(SIGVTALRM as u32).unwrap());
                        }
                        if timer.suspend_prof() {
                            thread.receive(Sig::from_user(SIGPROF as u32).unwrap());
                        }
                    }
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

/// `OutermostFuture`用来一劳永逸地管理用户线程的环境切换, 例如页表切换和关中断状态等.
struct OutermostFuture<F: Future + Send + 'static> {
    local_switch: LocalNow,
    future: F,
}
impl<F: Future + Send + 'static> OutermostFuture<F> {
    #[inline]
    pub fn new(thread: Arc<Thread>, future: F) -> Self {
        let page_table = thread
            .process
            .alive_then_uncheck(|a| a.user_space.page_table_arc());
        let local_switch = LocalNow::Task(Box::new(TaskLocal {
            always_local: AlwaysLocal::new(),
            thread,
            page_table,
        }));
        Self {
            local_switch,
            future,
        }
    }
}

impl<F: Future + Send + 'static> Future for OutermostFuture<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let local = local::hart_local();
        local.local_rcu.critical_start();
        local.handle();
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
        local.handle();
        ret
    }
}
