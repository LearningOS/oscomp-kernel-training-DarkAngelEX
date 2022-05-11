use core::{
    arch::{asm, global_asm},
    sync::atomic::{AtomicUsize, Ordering},
};

use riscv::register::{stvec, utvec::TrapMode};

use crate::{
    board::CLOCK_FREQ,
    hart::{sbi, sfence},
    sync::mutex::{SpinLock, SpinNoIrqLock},
    timer::{self, TimeTicks},
    trap,
    user::AutoSie,
};

const BENCHMARK: bool = false;

global_asm!(include_str!("benchmark.S"));

#[inline(always)]
fn black<T>(a: T) -> T {
    core::hint::black_box(a)
}
#[inline(always)]
fn black_usize(a: usize) -> usize {
    let ret;
    unsafe { asm!("mv {0}, {1}", out(reg)ret, in(reg)a) };
    ret
}

pub fn run_all() {
    frequent_test();
    if BENCHMARK {
        stack_trace!();
        println!("[FTL OS]branchmark_all begin");
        atomic_test();
        println!("[FTL OS]branchmark_all end");
        panic!("benchmark complete");
    }
}

pub struct BenchmarkTimer {
    tick: TimeTicks,
}

impl BenchmarkTimer {
    pub fn new() -> Self {
        Self {
            tick: timer::get_time_ticks(),
        }
    }
    // pub fn reset(&mut self) {
    //     self.tick = timer::get_time_ticks();
    // }
    #[inline(never)]
    pub fn check(&mut self, msg: &'static str, base_time: TimeTicks, ratio: usize) -> TimeTicks {
        let dur = TimeTicks::from_usize((timer::get_time_ticks() - self.tick).into_usize() * ratio);
        let base = dur.into_usize() * 100 / base_time.into_usize();
        println!("    {}", msg);
        print!(
            "        {:>5}ms {:>4}.{:0<2}",
            dur.into_millisecond(),
            base / 100,
            base % 100
        );
        let sep = 100;
        let mut n = base / sep;
        if n >= sep {
            n = sep + (n - sep) / 20;
        }
        print!("  ");
        for _i in 0..n.min(sep) * 5 / 10 {
            print!("*");
        }
        print!("\x1b[31m");
        for _i in sep..n {
            print!("*");
        }
        print!("\x1b[0m");
        println!();
        self.tick = timer::get_time_ticks();
        dur
    }
}

#[inline(never)]
fn frequent_test() {
    extern "C" {
        fn __time_frequent_test_impl(cnt: usize);
    }
    println!("[FTL OS]frequent test begin");
    const BASE: usize = 200_000_000;
    let start = timer::get_time_ticks();
    unsafe { __time_frequent_test_impl(BASE) };
    let dur = (timer::get_time_ticks() - start).into_usize();
    println!(
        "run {}M using {} ticks time: {}ms",
        BASE / 1000_000,
        dur,
        dur * 1000 / CLOCK_FREQ
    );
    print!("{}", to_yellow!());
    println!(
        "clock: {}KHz, core: {}MHz",
        CLOCK_FREQ / 1000,
        BASE * CLOCK_FREQ / dur / 1000_000
    );
    print!("{}", reset_color!());
}

#[inline(never)]
fn atomic_test() {
    let mut timer = BenchmarkTimer::new();
    let base_n = 100_000_000;
    let a = 0;
    for _i in 0..base_n {
        unsafe { asm!("") };
        // unsafe { asm!("add {0}, {0}, {1}", inout(reg)a, in(reg)i) };
        // let x = black_usize(i);
        // a += x;
    }
    black(a);
    let base_time = timer.check("native for", TimeTicks::from_millisecond(10), 1);

    let ratio = 1;
    let a = AtomicUsize::new(0);
    for i in 0..base_n / ratio {
        let x = black_usize(i);
        a.fetch_add(x, Ordering::Relaxed);
    }
    black(a);
    timer.check("atomic fetch_add", base_time, ratio);

    let ratio = 2;
    let a = SpinLock::new(0);
    for i in 0..base_n / ratio {
        let x = black_usize(i);
        *a.lock() += x;
    }
    black(a);
    timer.check("SpinLock add", base_time, ratio);

    let ratio = 4;
    let a = SpinNoIrqLock::new(0);
    for i in 0..base_n / ratio {
        let x = black_usize(i);
        *a.lock() += x;
    }
    black(a);
    timer.check("SpinNoIrqLock add", base_time, ratio);

    let ratio = 1;
    let a = AtomicUsize::new(0);
    for i in 0..base_n / ratio {
        let x = a.load(Ordering::Relaxed);
        a.compare_exchange_weak(x, x + i, Ordering::Relaxed, Ordering::Relaxed)
            .unwrap();
    }
    timer.check("CAS", base_time, ratio);

    let ratio = 3;
    for _i in 0..base_n / ratio {
        sfence::fence_i();
    }
    timer.check("fence_i", base_time, ratio);

    let ratio = 40;
    for _i in 0..base_n / ratio {
        sbi::remote_fence_i(0);
    }
    timer.check("remote_fence_i", base_time, ratio);

    let ratio = 4;
    let a = 0;
    for _i in 0..base_n / ratio {
        AutoSie::new();
    }
    black(a);
    timer.check("AutoSie effect", base_time, ratio);

    let ratio = 2;
    let a = AutoSie::new();
    for _i in 0..base_n / ratio {
        let _a = AutoSie::new();
    }
    drop(a);
    timer.check("AutoSie ignore", base_time, ratio);

    let ratio = 2;
    for _i in 0..base_n / ratio {
        let temp: usize = 0;
        unsafe { asm!("csrr {0}, sstatus", in(reg)temp) };
    }
    timer.check("sstatus read", base_time, ratio);

    let ratio = 2;
    let temp: usize;
    unsafe { asm!("csrr {0}, sstatus",out(reg)temp) };
    for _i in 0..base_n / ratio {
        unsafe { asm!( "csrw sstatus, {0}", in(reg)temp) };
    }
    timer.check("sstatus write", base_time, ratio);

    let ratio = 2;
    let temp: usize;
    unsafe { asm!("csrr {0}, satp",out(reg)temp) };
    for _i in 0..base_n / ratio {
        unsafe { asm!( "csrw satp, {0}", in(reg)temp) };
    }
    timer.check("satp write", base_time, ratio);

    if true {
        let ratio = 10;
        unsafe { set_benchmark_trap() };
        let ptr = 8 as *mut usize;
        println!("vector trap test start 0");
        unsafe { ptr.read_volatile() };
        println!("vector trap test start 1");
        for _i in 0..base_n / ratio {
            unsafe { ptr.read_volatile() };
        }
        unsafe { trap::set_kernel_default_trap() };
        timer.check("benchmark_trap", base_time, ratio);

        let ratio = 10;
        unsafe { set_benchmark_save_trap() };
        let ptr = 8 as *mut usize;
        for _i in 0..base_n / ratio {
            unsafe { ptr.read_volatile() };
        }
        unsafe { trap::set_kernel_default_trap() };
        timer.check("benchmark_save_trap", base_time, ratio);
    }

    let ratio = 50;
    for _i in 0..base_n / ratio {
        sfence::sfence_vma_all_global();
    }
    timer.check("sfence_vma_all_global", base_time, ratio);

    let ratio = 50;
    for _i in 0..base_n / ratio {
        sfence::sfence_vma_asid(123);
    }
    timer.check("sfence_vma_asid", base_time, ratio);

    let ratio = 50;
    for _i in 0..base_n / ratio {
        sfence::sfence_vma_va_asid(123, 456);
    }
    timer.check("sfence_vma_va_asid", base_time, ratio);
}

unsafe fn set_benchmark_trap() {
    extern "C" {
        fn __kernel_benchmark_vector();
    }
    stvec::write(__kernel_benchmark_vector as usize, TrapMode::Vectored);
}
unsafe fn set_benchmark_save_trap() {
    extern "C" {
        fn __kernel_benchmark_save_vector();
    }
    stvec::write(__kernel_benchmark_save_vector as usize, TrapMode::Vectored);
}
