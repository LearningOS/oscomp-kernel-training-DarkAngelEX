#![allow(dead_code)]
use crate::hart::sbi;

use crate::sync::mutex::SpinNoIrqLock;
use core::{
    fmt::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

const OUTPUT_LOCK: bool = true;

static ALLOW_GETCHAR: AtomicBool = AtomicBool::new(true);

pub fn init() {
    ftl_util::console::init(print);
}

struct Stdout;

#[inline(always)]
pub fn putchar(c: char) {
    sbi::console_putchar(c as usize);
}

#[inline(always)]
pub fn getchar() -> char {
    while ALLOW_GETCHAR
        .compare_exchange(true, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {}
    unsafe { char::from_u32_unchecked(sbi::console_getchar() as u32) }
}
pub fn disable_getchar() {
    ALLOW_GETCHAR.store(false, Ordering::SeqCst);
}
impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            putchar(c);
        }
        Ok(())
    }

    fn write_char(&mut self, c: char) -> fmt::Result {
        self.write_str(c.encode_utf8(&mut [0; 4]))
    }

    fn write_fmt(mut self: &mut Self, args: fmt::Arguments<'_>) -> fmt::Result {
        fmt::write(&mut self, args)
    }
}

#[link_section = "data"]
static WRITE_MUTEX: SpinNoIrqLock<Stdout> = SpinNoIrqLock::new(Stdout);

pub fn print(args: fmt::Arguments) {
    if OUTPUT_LOCK {
        WRITE_MUTEX.lock().write_fmt(args).unwrap();
    } else {
        Stdout.write_fmt(args).unwrap()
    }
}

pub fn print_unlocked(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap()
}

macro_rules! print_unlocked {
    ($fmt: literal $(, $($arg: tt)+)?) => {{
        $crate::console::print_unlocked(format_args!($fmt $(, $($arg)+)?));
    }}
}
