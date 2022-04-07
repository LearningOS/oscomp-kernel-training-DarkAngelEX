//! 处理用户态的异常

use core::convert::TryFrom;

use alloc::sync::Arc;
use riscv::register::scause::Exception;

use crate::{
    local,
    memory::{address::UserAddr, map_segment::handler::SpaceHolder, AccessType},
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
            thread.tid,
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
    let handle = || {
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
    };
    match handle() {
        Err(()) => user_fatal_error(),
        Ok(Ok((addr, asid))) => {
            if PRINT_PAGE_FAULT {
                println!("{}", to_green!("success handle exception"));
            }
            local::all_hart_sfence_vma_va_asid(addr, asid);
        }
        Ok(Err((addr, a))) => {
            stack_trace!();
            let sh = SpaceHolder::new(thread.process.clone());
            match a.a_page_fault(sh, addr).await {
                Ok((addr, asid)) => {
                    if PRINT_PAGE_FAULT {
                        println!("{}", to_green!("success handle exception by async"));
                    }
                    local::all_hart_sfence_vma_va_asid(addr, asid);
                }
                Err(_e) => user_fatal_error(),
            }
        }
    }
    do_exit
}
