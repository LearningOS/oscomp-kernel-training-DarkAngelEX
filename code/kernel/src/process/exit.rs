use alloc::sync::Weak;

use crate::{
    local, memory, process::Pid, signal::Sig, sync::even_bus::Event, xdebug::PRINT_SYSCALL_ALL,
};

use super::{children::ChildrenSet, search, thread::Thread, Process};

pub async fn exit_impl(thread: &Thread) {
    stack_trace!();
    let process = &*thread.process;
    let pid = process.pid();
    if PRINT_SYSCALL_ALL {
        print!("{}", to_green!());
        print!("thread {:?} {:?} exit", pid, thread.tid());
        println!("{}", reset_color!());
    }
    if !thread.inner().exited {
        print!("{}", to_red!());
        print!("thread {:?} {:?} terminal abnormally", pid, thread.tid());
        println!("{}", reset_color!());
    }
    debug_assert!(pid != Pid(0), "{}", to_red!("initproc exit"));
    thread.cleartid().await;
    let (parent, mut children);
    let asid;
    thread.timer_fence();
    {
        let mut lock = process.alive.lock();
        let alive = match lock.as_mut() {
            Some(a) => a,
            None => panic!(),
        };
        alive.threads.remove(thread.tid());
        if !alive.threads.is_empty() {
            return;
        }
        // 最后一个线程退出
        asid = alive.asid();
        process.event_bus.close();
        memory::set_satp_by_global();
        (parent, children) = alive.take_parent_children();
        stack_trace!();
        *lock = None; // 这里会释放进程页表
    }
    local::all_hart_sfence_vma_asid(asid);
    become_zomble(parent, pid, thread.exit_send_signal());
    throw_children(&mut children);
}

/// 在父进程中注册自身为僵尸并发送信号
fn become_zomble(parent: Option<Weak<Process>>, pid: Pid, sig: Option<Sig>) {
    let signal_event = match sig {
        Some(_) => Event::RECEIVE_SIGNAL,
        None => Event::EMPTY,
    };
    let evnet = signal_event | Event::CHILD_PROCESS_QUIT;
    match parent
        .and_then(|p| p.upgrade())
        .as_deref()
        .map(|p| (p, p.alive.lock()))
        .as_mut()
        .and_then(|(p, a)| a.as_mut().map(|a| (*p, a)))
    {
        Some((p, alive)) => {
            alive.children.become_zombie(pid);
            if let Some(s) = sig {
                p.signal_manager.receive(s)
            }
            let _ = p.event_bus.set(evnet);
        }
        _ => {
            let p = search::get_initproc();
            let mut alive = p.alive.lock();
            alive.as_mut().unwrap().children.become_zombie(pid);
            if let Some(s) = sig {
                p.signal_manager.receive(s)
            }
            let _ = p.event_bus.set(evnet);
        }
    };
}

/// 将子进程扔给初始进程
fn throw_children(children: &mut ChildrenSet) {
    if !children.is_empty() {
        let initproc = search::get_initproc();
        let mut initproc_alive = initproc.alive.lock();
        let ich = &mut initproc_alive.as_mut().unwrap().children;
        ich.append(children);
        if ich.have_zombies() {
            drop(initproc_alive);
            let _ = initproc.event_bus.set(Event::CHILD_PROCESS_QUIT);
        }
    }
}
