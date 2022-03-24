use crate::{hart::sbi, place};

use crate::sync::mutex::SpinNoIrqLock;
use core::{
    fmt::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
};

const OUTPUT_LOCK: bool = true;

static ALLOW_GETCHAR: AtomicBool = AtomicBool::new(true);

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
        WRITE_MUTEX.lock(place!()).write_fmt(args).unwrap();
    } else {
        Stdout.write_fmt(args).unwrap()
    }
}

pub fn print_unlock(args: fmt::Arguments) {
    Stdout.write_fmt(args).unwrap()
}

#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?));
    }
}

#[macro_export]
macro_rules! println {
    () => {
        $crate::console::print(format_args!("\n"));
    };
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }
}

#[macro_export]
macro_rules! print_unlock {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print_unlock(format_args!($fmt $(, $($arg)+)?));
    }
}
