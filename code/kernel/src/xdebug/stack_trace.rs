use core::fmt::Display;

use alloc::{string::String, vec::Vec};

use crate::{hart::cpu, local};

/// panic时打印堆栈上全部调用了stack_trace的路径
pub const STACK_TRACE: bool = true;

#[macro_export]
macro_rules! stack_trace {
    () => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::xdebug::stack_trace::XInfo::None,
            file!(),
            line!(),
        );
    };
    ($msg: literal) => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::xdebug::stack_trace::XInfo::Str($msg),
            file!(),
            line!(),
        );
    };
    ($msg: expr) => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::xdebug::stack_trace::XInfo::from($msg),
            file!(),
            line!(),
        );
    };
    ($msg: literal, $($arg:tt)*) => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::xdebug::stack_trace::XInfo::String(alloc::format!($msg, $($arg)*)),
            file!(),
            line!(),
        );
    };
}

pub struct StackTracker;

impl StackTracker {
    #[inline(always)]
    pub fn new(msg: XInfo, file: &'static str, line: u32) -> Self {
        if STACK_TRACE {
            let info = StackInfo::new(msg, file, line);
            local::always_local().stack_trace.push(info);
        }
        Self
    }
}

impl Drop for StackTracker {
    #[inline(always)]
    fn drop(&mut self) {
        if STACK_TRACE {
            local::always_local().stack_trace.pop();
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

pub struct StackInfo {
    hart: usize,
    msg: XInfo,
    file: &'static str,
    line: u32,
}

impl StackInfo {
    pub fn new(msg: XInfo, file: &'static str, line: u32) -> Self {
        Self {
            hart: cpu::hart_id(),
            msg,
            file,
            line,
        }
    }
    pub fn show(&self, i: usize) {
        println!(
            "{} hart {} {}:{} {}",
            i, self.hart, self.file, self.line, self.msg,
        );
    }
}

pub struct StackTrace {
    stack: Vec<StackInfo>,
}

impl StackTrace {
    pub const fn new() -> Self {
        Self { stack: Vec::new() }
    }
    pub fn clear(&mut self) {
        self.stack.clear()
    }
    pub fn push(&mut self, info: StackInfo) {
        self.stack.push(info);
    }
    pub fn pop(&mut self) {
        self.stack.pop();
    }
    pub fn print_all_stack(&self) {
        for (i, info) in self.stack.iter().rev().enumerate() {
            info.show(i)
        }
    }
}
