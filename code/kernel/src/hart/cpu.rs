use core::{
    arch::asm,
    sync::atomic::{AtomicUsize, Ordering},
};

static CPU_COUNT: AtomicUsize = AtomicUsize::new(0);

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

pub unsafe fn increase_cpu() {
    CPU_COUNT.fetch_add(1, Ordering::Relaxed);
}
pub fn count() -> usize {
    CPU_COUNT.load(Ordering::Relaxed)
}
