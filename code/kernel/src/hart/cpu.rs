use core::{
    arch::asm,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::local;

static CPU_COUNT: AtomicUsize = AtomicUsize::new(0);
static CPU_MIN: AtomicUsize = AtomicUsize::new(usize::MAX);
static CPU_MAX: AtomicUsize = AtomicUsize::new(0);

pub unsafe fn set_hart_local(hartid: usize) {
    let hart_local = local::bind_tp(hartid);
    asm!("mv tp, {}", in(reg) hart_local);
}

#[inline(always)]
pub fn get_tp() -> usize {
    unsafe {
        let ret;
        asm!("mv {}, tp", out(reg) ret);
        ret
    }
}

pub unsafe fn set_gp() {
    asm!(
        "
    .option push
    .option norelax
    la gp, __global_pointer$
    .option pop
    "
    );
}

#[inline(always)]
pub fn hart_id() -> usize {
    local::hart_local().cpuid()
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
pub fn hart_range() -> Range<usize> {
    CPU_MIN.load(Ordering::Relaxed)..CPU_MAX.load(Ordering::Relaxed) + 1
}
