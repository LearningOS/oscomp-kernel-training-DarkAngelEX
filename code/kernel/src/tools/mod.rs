#![allow(dead_code)]

use core::{
    arch::asm,
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::string::String;
use riscv::register::sstatus;

use crate::{hart::cpu, timer};
#[macro_use]
pub mod color;
#[macro_use]
pub mod allocator;
pub mod container;
pub mod error;
pub mod range;
pub mod xasync;

pub const fn bool_result(x: bool) -> Result<(), ()> {
    if x {
        Ok(())
    } else {
        Err(())
    }
}

#[macro_export]
macro_rules! impl_usize_from {
    ($name: ty, $v: ident, $body: stmt) => {
        impl From<$name> for usize {
            fn from($v: $name) -> Self {
                $body
            }
        }
        impl $name {
            pub const fn into_usize(self) -> usize {
                let $v = self;
                $body
            }
        }
    };
}

pub struct FailRun<T: FnOnce()> {
    drop_run: Option<T>,
}

impl<T: FnOnce()> Drop for FailRun<T> {
    fn drop(&mut self) {
        if let Some(f) = self.drop_run.take() {
            f()
        }
    }
}

impl<T: FnOnce()> FailRun<T> {
    pub fn new(f: T) -> Self {
        Self { drop_run: Some(f) }
    }
    pub fn consume(mut self) {
        self.drop_run = None;
    }
}

pub trait Wrapper<T> {
    type Output;
    fn wrapper(a: T) -> Self::Output;
}

#[derive(Clone)]
pub struct ForwardWrapper;
impl<T> Wrapper<T> for ForwardWrapper {
    type Output = T;
    fn wrapper(a: T) -> T {
        a
    }
}

pub fn size_to_mkb(size: usize) -> (usize, usize, usize) {
    let mask = 1 << 10;
    (size >> 20, (size >> 10) % mask, size % mask)
}

pub fn next_instruction_sepc(sepc: usize, ir: u8) -> usize {
    if ir & 0b11 == 0b11 {
        sepc + 4
    } else {
        sepc + 2 //  RVC extend: Compressed Instructions
    }
}

pub fn next_sepc(sepc: usize) -> usize {
    let ir = unsafe { *(sepc as *const u8) };
    next_instruction_sepc(sepc, ir)
}

static BLOCK_0: AtomicUsize = AtomicUsize::new(0);
static BLOCK_1: AtomicUsize = AtomicUsize::new(0);
static BLOCK_2: AtomicUsize = AtomicUsize::new(0);

pub fn wait_all_hart_impl(target: &AtomicUsize) {
    target.fetch_add(1, Ordering::SeqCst);
    let cpu_count = cpu::count();
    let mut x = 0;
    loop {
        let cur = target.load(Ordering::SeqCst);
        if cur == cpu_count {
            break;
        }
        x += 1;
        if x % 1000000 == 0 {
            // panic!("deadlock: {}", cur);
        }
    }
}
pub fn wait_all_hart() {
    wait_all_hart_impl(&BLOCK_0);
    BLOCK_2.store(0, Ordering::SeqCst);
    wait_all_hart_impl(&BLOCK_1);
    BLOCK_0.store(0, Ordering::SeqCst);
    wait_all_hart_impl(&BLOCK_2);
    BLOCK_1.store(0, Ordering::SeqCst);
}

pub fn n_space(n: usize) -> String {
    let mut s = String::new();
    for _i in 0..n {
        s.push(' ');
    }
    s
}

const COLOR_TEST: bool = false;
const MULTI_THREAD_PERFORMANCE_TEST: bool = false;
const MULTI_THREAD_STRESS_TEST: bool = false;

pub fn multi_thread_test(hart: usize) {
    if COLOR_TEST {
        wait_all_hart();
        if hart == 0 {
            color::test::color_test();
        }
    }
    wait_all_hart();
    multi_thread_performance_test(hart);
    multi_thread_stress_test(hart);
}

fn multi_thread_performance_test(hart: usize) {
    if MULTI_THREAD_PERFORMANCE_TEST {
        assert!(!sstatus::read().sie());
        wait_all_hart();
        container::multi_thread_performance_test(hart);
        wait_all_hart();
        panic!("multi_thread_performance_test complete");
    } else if false {
        if hart == 0 {
            let mut cnt = 0x1000000;
            loop {
                let begin = timer::get_time_ticks();
                for _i in 0..cnt {
                    unsafe { asm!("nop") }
                }
                let end = timer::get_time_ticks();
                let total = (end - begin).into_millisecond();
                println!("loop {:#x}({}) using {}ms", cnt, cnt, total);
                if total > 1000 {
                    break;
                }
                cnt *= 2;
            }
        }
        wait_all_hart();
    } else {
        if hart == 0 {
            println!("skip multi_thread_performance_test");
        }
    }
}

fn multi_thread_stress_test(hart: usize) {
    if MULTI_THREAD_STRESS_TEST {
        assert!(!sstatus::read().sie());
        wait_all_hart();
        container::multi_thread_stress_test(hart);
        wait_all_hart();
        panic!("multi_thread_stress_test complete");
    } else {
        wait_all_hart();
        if hart == 0 {
            println!("skip multi_thread_stress_test");
        }
    }
}
