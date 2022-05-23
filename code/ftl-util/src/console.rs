#![allow(dead_code)]
use core::fmt;

static mut WRITE_FN: Option<fn(fmt::Arguments)> = None;

pub fn init(write_fn: fn(fmt::Arguments)) {
    unsafe {
        WRITE_FN.replace(write_fn);
    }
}

#[inline(always)]
pub fn print(args: fmt::Arguments) {
    match unsafe { WRITE_FN } {
        Some(write_fn) => write_fn(args),
        #[cfg(not(debug_assertions))]
        None => core::hint::unreachable_unchecked(),
        #[cfg(debug_assertions)]
        None => unimplemented!(),
    }
}

#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {{
        $crate::console::print(format_args!($fmt $(, $($arg)+)?));
    }}
}

#[macro_export]
macro_rules! println {
    () => {{
        $crate::console::print(format_args!("\n"));
    }};
    ($fmt: literal $(, $($arg: tt)+)?) => {{
        $crate::console::print(format_args!(concat!($fmt, "\n") $(, $($arg)+)?));
    }}
}
