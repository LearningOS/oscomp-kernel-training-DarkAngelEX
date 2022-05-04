use core::fmt::Display;

use alloc::string::String;

/// panic时打印堆栈上全部调用了stack_trace的路径
pub const STACK_TRACE: bool = true;

static mut STACK_PUSH_FN: Option<fn(XInfo, &'static str, u32)> = None;
static mut STACK_POP_FN: Option<fn()> = None;

pub fn init(push_fn: fn(XInfo, &'static str, u32), pop_fn: fn()) {
    unsafe {
        STACK_PUSH_FN.replace(push_fn);
        STACK_POP_FN.replace(pop_fn);
    }
}

#[macro_export]
#[cfg(feature = "stack_trace")]
macro_rules! stack_trace {
    // stack_trace!()
    () => {
        let _stack_trace = $crate::xdebug::stack::StackTracker::new(
            $crate::xdebug::stack::XInfo::None,
            file!(),
            line!(),
        );
    };
    // stack_trace!("message")
    ($msg: literal) => {
        let _stack_trace = $crate::xdebug::stack::StackTracker::new(
            $crate::xdebug::stack::XInfo::Str($msg),
            file!(),
            line!(),
        );
    };
    // stack_trace!(123456)
    ($msg: expr) => {
        let _stack_trace = $crate::xdebug::stack::StackTracker::new(
            $crate::xdebug::stack::XInfo::from($msg),
            file!(),
            line!(),
        );
    };
    // stack_trace!("{}", abc)
    ($msg: literal, $($arg:tt)*) => {
        let _stack_trace = $crate::xdebug::stack::StackTracker::new(
            $crate::xdebug::stack::XInfo::String(alloc::format!($msg, $($arg)*)),
            file!(),
            line!(),
        );
    };
}
#[cfg(not(feature = "stack_trace"))]
macro_rules! stack_trace {
    () => {};
    ($msg: literal) => {};
    ($msg: expr) => {};
    ($msg: literal, $($arg:tt)*) => {};
}

pub struct StackTracker;

impl StackTracker {
    #[inline(always)]
    pub fn new(msg: XInfo, file: &'static str, line: u32) -> Self {
        match unsafe { STACK_PUSH_FN } {
            #[cfg(feature = "stack_trace")]
            Some(f) => f(msg, file, line),
            _ => drop((msg, file, line)),
        }
        Self
    }
}

impl Drop for StackTracker {
    #[inline(always)]
    fn drop(&mut self) {
        match unsafe { STACK_POP_FN } {
            #[cfg(feature = "stack_trace")]
            Some(f) => f(),
            _ => (),
        }
    }
}

pub enum XInfo {
    None,
    Str(&'static str),
    Number(usize),
    String(String),
}
impl From<usize> for XInfo {
    fn from(a: usize) -> Self {
        Self::Number(a)
    }
}
impl From<&'static str> for XInfo {
    fn from(s: &'static str) -> Self {
        Self::Str(s)
    }
}
impl Display for XInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            XInfo::None => Ok(()),
            XInfo::Str(s) => f.write_str(s),
            XInfo::Number(x) => write!(f, "{:#x}", x),
            XInfo::String(s) => f.write_str(s),
        }
    }
}
