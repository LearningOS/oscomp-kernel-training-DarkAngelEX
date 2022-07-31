use alloc::vec::Vec;
use ftl_util::xdebug::stack::XInfo;

use crate::{hart::cpu, local, user::NativeAutoSie};

pub fn init() {
    ftl_util::xdebug::stack::init(
        |msg, file, line| {
            let info = StackInfo::new(msg, file, line);
            // 关中断防止中断处理程序干涉操作过程
            let _sie = NativeAutoSie::new();
            local::always_local().stack_trace.push(info);
        },
        || {
            // 关中断防止中断处理程序干涉操作过程
            let _sie = NativeAutoSie::new();
            local::always_local().stack_trace.pop();
        },
    );
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
