#![allow(dead_code)]

use crate::riscv::sbi;

use core::fmt::{self, Write};

struct Stdout;

use crate::sync::mutex::SpinLock;

#[inline(always)]
pub fn putchar(c: char) {
    sbi::console_putchar(c as usize);
}

#[inline(always)]
pub fn getchar() -> char {
    unsafe { char::from_u32_unchecked(sbi::console_getchar() as u32) }
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

static WRITE_MUTEX: SpinLock<Stdout> = SpinLock::new(Stdout);

pub fn print(args: fmt::Arguments) {
    WRITE_MUTEX.lock().write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!($fmt $(, $($arg)+)?));
    }
}

#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }
}
