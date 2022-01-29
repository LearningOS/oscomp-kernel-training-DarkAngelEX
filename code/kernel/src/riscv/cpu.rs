use core::arch::asm;

pub unsafe fn set_cpu_id(cpu_id: usize) {
    asm!("mv tp, {}", in(reg) cpu_id);
}

pub fn hart_id() -> usize {
    let cpu_id;
    unsafe {
        asm!("mv {}, tp", lateout(reg) cpu_id);
    }
    cpu_id
}


