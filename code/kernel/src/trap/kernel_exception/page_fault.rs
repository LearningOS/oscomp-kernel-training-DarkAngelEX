use core::convert::TryFrom;

use riscv::register::scause::Exception;

use crate::{
    local, memory::address::UserAddr, tools, trap::kernel_exception::fatal_exception_error,
};

pub fn page_fault_handle(e: Exception, stval: usize, mut sepc: usize) -> usize {
    let mut error = true;
    stack_trace!();
    let local = local::always_local();

    if local.sum_cur() != 0 {
        assert!(local.user_access_status.not_forbid());
        if stval >= 0x1000 && let Ok(addr) = UserAddr::try_from(stval as *const u8) {
            if local.user_access_status.is_access() {
                println!(
                    "access user data error! ignore this instruction. {:?} stval: {:#x}",
                    e, stval
                );
                local.user_access_status.set_error(addr, e);
            }
            sepc = tools::next_sepc(sepc);
            error = false;
        }
    } else {
        assert!(local.user_access_status.is_forbid());
    }
    if error {
        fatal_exception_error(0)
    }
    sepc
}
