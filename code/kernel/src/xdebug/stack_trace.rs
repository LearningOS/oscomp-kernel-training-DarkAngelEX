use alloc::vec::Vec;

use crate::riscv::cpu;

pub const STACK_TRACE: bool = true;

#[macro_export]
macro_rules! stack_trace_begin {
    () => {
        unsafe { (&mut *crate::scheduler::get_current_stack_trace()).clear() }
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::scheduler::try_get_current_stack_trace(),
            "",
            file!(),
            line!(),
        );
    };
    ($msg: expr) => {
        unsafe { (&mut *crate::scheduler::get_current_stack_trace()).clear() }
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::scheduler::try_get_current_stack_trace(),
            $msg,
            file!(),
            line!(),
        );
    };
}

#[macro_export]
macro_rules! stack_trace {
    () => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::scheduler::try_get_current_stack_trace(),
            "",
            file!(),
            line!(),
        );
    };
    ($msg: expr) => {
        let _stack_trace = crate::xdebug::stack_trace::StackTracker::new(
            crate::scheduler::try_get_current_stack_trace(),
            $msg,
            file!(),
            line!(),
        );
    };
}

pub struct StackTracker {
    trace: *mut StackTrace,
}

impl StackTracker {
    pub fn new(
        trace: Option<*mut StackTrace>,
        msg: &'static str,
        file: &'static str,
        line: u32,
    ) -> Option<Self> {
        if STACK_TRACE {
            trace.map(|trace| {
                unsafe { (*trace).push(StackInfo::new(msg, file, line)) }
                Self { trace }
            })
        } else {
            None
        }
    }
}

impl Drop for StackTracker {
    fn drop(&mut self) {
        unsafe { (*self.trace).pop() }
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
        println!("{} {} {}:{} hart {}", i, self.msg, self.file, self.line, cpu::hart_id());
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
}
