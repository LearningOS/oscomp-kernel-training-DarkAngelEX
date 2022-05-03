#![allow(dead_code)]
use core::fmt::{self, Write};

#[inline(always)]
pub fn console_putchar(c: char) {
    extern "C" {
        fn global_console_putchar(c: usize);
    }
    unsafe { global_console_putchar(c as usize) };
}
extern "C" {
    fn global_console_lock();
    fn global_console_unlock();
}

struct Stdout;

impl Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            console_putchar(c);
        }
        Ok(())
    }
}

pub fn print(args: fmt::Arguments) {
    unsafe {
        global_console_lock();
        Stdout.write_fmt(args).unwrap();
        global_console_unlock();
    }
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
