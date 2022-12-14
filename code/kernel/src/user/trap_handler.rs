//! 处理用户态的异常

use core::convert::TryFrom;

use riscv::register::scause::Exception;

use crate::{
    local,
    memory::{address::UserAddr, allocator::frame, AccessType},
    process::thread::Thread,
    signal::{Action, Sig, SIGSEGV},
    tools::xasync::TryRunFail,
    xdebug::PRINT_PAGE_FAULT,
};

// return do_exit
pub async fn page_fault(thread: &Thread, e: Exception, stval: usize, sepc: usize) -> bool {
    let mut do_exit = false;
    let mut user_fatal_error = || {
        println!(
            "[kernel]user_fatal_error page_fault {:?} {:?} {:?} stval: {:#x} sepc: {:#x} ra: {:#x}",
            thread.process.pid(),
            thread.tid(),
            e,
            stval,
            sepc,
            thread.get_context().ra()
        );
        if stval != sepc {
            print!("error IR: ");
            let _sum = crate::user::AutoSum::new();
            for i in 0..8 {
                let p = sepc + i;
                print!("{:0>2x} ", unsafe { *(p as *const u8) });
            }
            println!();
            // println!("a0-a7: {:#x?}", thread.get_context().a0_a7());
            // println!("all: {:#x?}", thread.get_context().user_rx);
            println!();
        }
        do_exit = true;
    };
    if PRINT_PAGE_FAULT {
        println!(
            "{}{:?} {:?} staval {:#x}{}",
            to_yellow!(),
            thread.process.pid(),
            e,
            stval,
            reset_color!()
        );
    }
    let mut exec = false;
    let mut rv = || {
        stack_trace!();
        let addr = UserAddr::try_from(stval as *const u8)?;
        let perm = AccessType::from_exception(e).unwrap();
        exec = perm.exec;
        let addr = addr.floor();
        let allocator = &mut frame::default_allocator();
        match thread
            .process
            .alive_then(|a| a.user_space.page_fault(addr, perm, allocator))
        {
            Ok(x) => Ok(Ok(x)),
            Err(TryRunFail::Async(a)) => Ok(Err((addr, a))),
            Err(TryRunFail::Error(e)) => Err(e),
        }
    };
    let mut handle_fail = false;
    match rv() {
        Err(_e) => handle_fail = true,
        Ok(Ok(flush)) => {
            if PRINT_PAGE_FAULT {
                println!("{}", to_green!("success handle exception"));
            }
            flush.run();
        }
        Ok(Err((addr, a))) => {
            stack_trace!();
            match a.a_page_fault(&thread.process, addr).await {
                Ok(flush) => {
                    flush.run();
                    if PRINT_PAGE_FAULT {
                        println!("{}", to_green!("success handle exception by async"));
                    }
                }
                Err(_e) => handle_fail = true,
            }
        }
    }
    if handle_fail {
        let segv = Sig::from_user(SIGSEGV as u32).unwrap();
        match thread.process.signal_manager.get_action(segv).0 {
            Action::Handler(_, _) => thread.receive(segv),
            _ => user_fatal_error(),
        }
    } else if exec {
        local::all_hart_fence_i();
    }
    do_exit
}
