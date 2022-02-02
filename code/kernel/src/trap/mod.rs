use core::arch::global_asm;

pub mod context;
global_asm!(include_str!("trap.S"));


/// return value is sscratch = ptr of TrapContext
#[no_mangle]
pub fn trap_handler() -> usize {
    todo!()
}
