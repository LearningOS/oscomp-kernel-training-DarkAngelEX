#![allow(dead_code)]

pub const PRINT_MAP_ALL: bool = false;

pub const GLOBAL_DEBUG: bool = true;

pub const PRINT_TRAP: bool = false;

pub const PRINT_SCHEDULER: bool = false;

pub const PRINT_FORK: bool = false;
pub const PRINT_SYSCALL: bool = false;
pub const PRINT_SYSCALL_ALL: bool = false;
// fork return and exec return
pub const PRINT_SPECIAL_RETURN: bool = false;
// check drop when becomes zombie
pub const PRINT_DROP_TCB: bool = false;

pub const PRINT_PAGE_FAULT: bool = false;

pub const CLOSE_FRAME_DEALLOC: bool = false;
pub const CLOSE_HEAP_DEALLOC: bool = false;
pub const CLOSE_LOCAL_HEAP: bool = true;

pub const FRAME_DEALLOC_OVERWRITE: bool = true;
pub const HEAP_DEALLOC_OVERWRITE: bool = true;

pub const CLOSE_TIME_INTERRUPT: bool = false;

#[macro_use]
pub mod trace;
#[macro_use]
pub mod stack_trace;

#[macro_export]
macro_rules! place {
    () => {
        concat!(file!(), ":", line!(), ":", column!())
    };
}

#[macro_export]
macro_rules! debug_run {
    () => {};
    ($x: block) => {
        if crate::xdebug::GLOBAL_DEBUG {
            $x;
        }
    };
}

#[macro_export]
macro_rules! debug_check {
    ($($arg:tt)*) => {
        if crate::xdebug::GLOBAL_DEBUG { assert!($($arg)*); }
    }
}

#[macro_export]
macro_rules! debug_check_eq {
    ($($arg:tt)*) => {
        if crate::xdebug::GLOBAL_DEBUG { assert_eq!($($arg)*); }
    }
}
#[macro_export]
macro_rules! debug_check_ne {
    ($($arg:tt)*) => {
        if crate::xdebug::GLOBAL_DEBUG { assert_ne!($($arg)*); }
    }
}

#[macro_export]
macro_rules! deubg_print_place {
    () => {
        println!("{}:{}", file!(), line!());
    };
    ($str: expr) => {
        println!("{}:{} {}", file!(), line!(), $str);
    };
}

/// NeverFail will panic unless run assume_success.
///
/// it's only used as a marker.
pub struct NeverFail;
impl Drop for NeverFail {
    fn drop(&mut self) {
        panic!("give up");
    }
}

impl NeverFail {
    pub fn new() -> Self {
        NeverFail
    }
    pub fn assume_success(self) {
        core::mem::forget(self)
    }
}
