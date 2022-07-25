#![allow(dead_code)]
use core::fmt::{self, Write};

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
        #[cfg(feature = "libc_output")]
        None => libc_output(args),
        #[cfg(not(debug_assertions))]
        None => unsafe { core::hint::unreachable_unchecked() },
        #[cfg(debug_assertions)]
        None => unimplemented!(),
    }
}

#[cfg(feature = "libc_output")]
fn libc_output(args: fmt::Arguments) {
    TestWrite.write_fmt(args).unwrap();
}

struct TestWrite;

impl Write for TestWrite {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        #[cfg(feature = "libc_output")]
        {
            extern "C" {
                fn putchar(fmt: u8);
            }
            for c in s.bytes() {
                unsafe { putchar(c) };
            }
            Ok(())
        }
        #[cfg(not(feature = "libc_output"))]
        {
            panic!("no_std output {}", s)
        }
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

#[test]
fn test_output() {
    println!("12345");
}
