use core::pin::Pin;

use alloc::vec::Vec;

use crate::hart::cpu;

pub const STACK_TRACE: bool = true;

#[macro_export]
macro_rules! stack_trace {
    ($stack_trace: expr) => {
        let _stack_trace =
            crate::xdebug::stack_trace::StackTracker::new($stack_trace, "", file!(), line!());
    };
}

pub struct StackTracker {
    trace_ptr: usize,
}

impl StackTracker {
    pub fn new(trace_ptr: usize, _msg: &'static str, _file: &'static str, _line: u32) -> Option<Self> {
        if STACK_TRACE {
            Some(Self { trace_ptr })
        } else {
            None
        }
    }
}

impl Drop for StackTracker {
    fn drop(&mut self) {
        unsafe { (*(self.trace_ptr as *mut StackTrace)).pop() }
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
            "{} {} {}:{} hart {}",
            i,
            self.msg,
            self.file,
            self.line,
            cpu::hart_id()
        );
    }
}

pub struct StackTrace {
    stack: Vec<StackInfo>,
}

impl StackTrace {
    pub fn new() -> Self {
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
