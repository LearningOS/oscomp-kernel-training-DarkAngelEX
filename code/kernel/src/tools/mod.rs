#![allow(dead_code)]

use core::{
    arch::asm,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};

use alloc::string::String;
use ftl_util::sync::{qspinlock::QSpinLock, spin_mutex::SpinMutex, Spin};
use riscv::register::sstatus;

use crate::{hart::cpu, timer};
#[macro_use]
pub mod color;
#[macro_use]
pub mod allocator;
pub mod container;
pub mod error;
pub mod path;
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

pub struct DynDropRun<T>(Option<(T, fn(T))>);
impl<T> Drop for DynDropRun<T> {
    #[inline(always)]
    fn drop(&mut self) {
        if let Some((v, f)) = self.0.take() {
            f(v)
        }
    }
}
impl<T> DynDropRun<T> {
    pub fn new(v: T, f: fn(T)) -> Self {
        Self(Some((v, f)))
    }
    pub fn run(self) {
        drop(self)
    }
    pub fn cancel(mut self) {
        self.0.take();
    }
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

#[repr(align(64))]
pub struct AlignCacheWrapper<T>(T);
impl<T> AlignCacheWrapper<T> {
    pub fn new(v: T) -> Self {
        Self(v)
    }
}

impl<T> Deref for AlignCacheWrapper<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for AlignCacheWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
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
const MULTI_THREAD_SPINLOCK_TEST: bool = false;

static HART_ALLOC: AtomicUsize = AtomicUsize::new(0);

pub fn multi_thread_test(hart: usize) {
    if COLOR_TEST {
        wait_all_hart();
        if hart == 0 {
            color::test::color_test();
        }
    }
    let hart = HART_ALLOC.fetch_add(1, Ordering::Relaxed);
    wait_all_hart();
    multi_thread_performance_test(hart);
    multi_thread_stress_test(hart);
    multi_thread_spin_lock_test(hart);
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
                let begin = timer::now();
                for _i in 0..cnt {
                    unsafe { asm!("nop") }
                }
                let end = timer::now();
                let total = (end - begin).as_millis();
                println!("loop {:#x}({}) using {}ms", cnt, cnt, total);
                if total > 1000 {
                    break;
                }
                cnt *= 2;
            }
        }
        wait_all_hart();
    } else {
        #[allow(clippy::collapsible_else_if)]
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

pub fn xor_shift_128_plus((s0, s1): (u64, u64)) -> (u64, u64) {
    let mut s2 = s0;
    s2 ^= s2 << 23;
    s2 ^= s1 ^ (s2 >> 18) ^ (s1 >> 5);
    (s1, s2)
}

struct SpinLockTest {
    locked: bool,
    count: usize,
}
impl SpinLockTest {
    const fn new() -> Self {
        Self {
            locked: false,
            count: 0,
        }
    }
    fn init(&mut self) {
        self.locked = false;
        self.count = 0;
    }
    fn peried_start(&mut self) -> usize {
        unsafe {
            assert!(!core::ptr::read_volatile(&self.locked));
            core::ptr::write_volatile(&mut self.locked, true);
            core::ptr::read_volatile(&self.count)
        }
    }
    #[inline(never)]
    fn peried_op(&mut self) {
        for i in 0..1 {
            core::hint::black_box(i);
        }
    }
    fn peried_end(&mut self, value: usize) {
        unsafe {
            assert!(core::ptr::read_volatile(&self.locked));
            core::ptr::write_volatile(&mut self.locked, false);
            assert_eq!(core::ptr::read_volatile(&self.count), value);
            core::ptr::write_volatile(&mut self.count, value + 1);
        }
    }
}

static OLD_SPINLOCK: SpinMutex<SpinLockTest, Spin> = SpinMutex::new(SpinLockTest::new());
static NEW_SPINLOCK: QSpinLock<SpinLockTest, Spin> = QSpinLock::new(SpinLockTest::new());

fn multi_thread_spin_lock_test(hart: usize) {
    if !MULTI_THREAD_SPINLOCK_TEST {
        return;
    }
    wait_all_hart();
    println!("hart {} ready", hart);
    wait_all_hart();
    run_lock(hart, || OLD_SPINLOCK.lock(), "OLD_SPINLOCK");
    run_lock(hart, || NEW_SPINLOCK.lock(), "NEW_SPINLOCK");
    run_lock(hart, || OLD_SPINLOCK.lock(), "OLD_SPINLOCK");
    run_lock(hart, || NEW_SPINLOCK.lock(), "NEW_SPINLOCK");
    run_lock(hart, || OLD_SPINLOCK.lock(), "OLD_SPINLOCK");
    run_lock(hart, || NEW_SPINLOCK.lock(), "NEW_SPINLOCK");
    wait_all_hart();
    panic!("test complete");
}

fn run_lock(
    hart: usize,
    f: impl Fn<(), Output = impl DerefMut<Target = SpinLockTest>>,
    msg: &'static str,
) {
    let n = 100000;
    wait_all_hart();
    let start = timer::now();
    for _ in 0..n {
        let mut lk = f();
        let v = lk.peried_start();
        lk.peried_op();
        lk.peried_end(v);
    }
    wait_all_hart();
    let end = timer::now();
    let target = n * cpu::count();
    if hart == 0 {
        println!(
            "{} target: {} get: {} time: {}ms",
            msg,
            target,
            f().count,
            (end - start).as_millis()
        );
    }
}
