//! 处理用户态的异常

use core::convert::TryFrom;

use alloc::sync::Arc;
use riscv::register::scause::Exception;

use crate::{
    memory::{address::UserAddr, AccessType},
    process::thread::Thread,
    tools::xasync::TryRunFail,
    xdebug::PRINT_PAGE_FAULT,
};

// return do_exit
pub async fn page_fault(thread: &Arc<Thread>, e: Exception, stval: usize, sepc: usize) -> bool {
    let mut do_exit = false;
    let mut user_fatal_error = || {
        println!(
            "[kernel]user_fatal_error {:?} {:?} {:?} stval: {:#x} sepc: {:#x}",
            thread.process.pid(),
            thread.tid(),
            e,
            stval,
            sepc
        );
        if stval != sepc {
            print!("error IR: ");
            let _sum = crate::user::AutoSum::new();
            for i in 0..8 {
                let p = sepc + i;
                print!("{:0>2x} ", unsafe { *(p as *const u8) });
            }
            println!();
            println!("a0-a7: {:#x?}", thread.get_context().a0_a7());
            println!("all: {:#x?}", thread.get_context().user_rx);
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
    let rv = {
        || {
            stack_trace!();
            let addr = UserAddr::try_from(stval as *const u8).map_err(|_| ())?;
            let perm = AccessType::from_exception(e).unwrap();
            let addr = addr.floor();
            match thread
                .process
                .alive_then(|a| a.user_space.page_fault(addr, perm))
                .map_err(|_| ())?
            {
                Ok(x) => Ok(Ok(x)),
                Err(TryRunFail::Async(a)) => Ok(Err((addr, a))),
                Err(TryRunFail::Error(_e)) => Err(()),
            }
        }
    }();
    match rv {
        Err(()) => user_fatal_error(),
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
                Err(_e) => user_fatal_error(),
            }
        }
    }
    do_exit
}
