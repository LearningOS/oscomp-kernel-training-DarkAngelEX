use core::{
    arch::asm,
    ops::RangeInclusive,
    sync::atomic::{AtomicUsize, Ordering},
};

static CPU_COUNT: AtomicUsize = AtomicUsize::new(0);
static CPU_MIN: AtomicUsize = AtomicUsize::new(usize::MAX);
static CPU_MAX: AtomicUsize = AtomicUsize::new(0);

pub unsafe fn set_cpu_id(cpu_id: usize) {
    asm!("mv tp, {}", in(reg) cpu_id);
}

pub unsafe fn set_gp() {
    asm!("
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    ");
}

#[inline(always)]
pub fn hart_id() -> usize {
    let cpu_id;
    unsafe {
        asm!("mv {}, tp", lateout(reg) cpu_id);
    }
    cpu_id
}

pub unsafe fn init(hart_id: usize) {
    CPU_COUNT.fetch_add(1, Ordering::Relaxed);
    CPU_MIN.fetch_min(hart_id, Ordering::Relaxed);
    CPU_MAX.fetch_max(hart_id, Ordering::Relaxed);
}

#[inline(always)]
pub fn count() -> usize {
    CPU_COUNT.load(Ordering::Relaxed)
}
#[inline(always)]
pub fn hart_range() -> RangeInclusive<usize> {
    CPU_MIN.load(Ordering::Relaxed)..=CPU_MAX.load(Ordering::Relaxed)
}
