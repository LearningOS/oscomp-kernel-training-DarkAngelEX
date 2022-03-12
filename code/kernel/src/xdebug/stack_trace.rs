use core::pin::Pin;

use alloc::vec::Vec;

use crate::{hart::cpu, local};

/// panic时打印堆栈上全部调用了stack_trace的路径
pub const STACK_TRACE: bool = true;

#[macro_export]
macro_rules! stack_trace {
    () => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new("", file!(), line!());
    };
    ($msg: expr) => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new($msg, file!(), line!());
    };
}

pub struct StackTracker;

impl StackTracker {
    #[inline(always)]
    pub fn new(msg: &'static str, file: &'static str, line: u32) -> Self {
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

#[derive(Clone, Copy)]
pub struct StackInfo {
    msg: &'static str,
    file: &'static str,
    line: u32,
}

impl StackInfo {
    pub fn new(msg: &'static str, file: &'static str, line: u32) -> Self {
        Self { msg, file, line }
    }
    pub fn show(&self, i: usize) {
        println!(
            "{} hart {} {} {}:{}",
            i,
            cpu::hart_id(),
            self.msg,
            self.file,
            self.line,
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
    pub fn ptr_usize(self: &mut Pin<&mut Self>) -> usize {
        &mut **self as *mut _ as usize
    }
}
