#![allow(dead_code)]
pub const STACK_TRACE: bool = true;

#[macro_export]
macro_rules! stack_trace {
    () => {
        let _stack_trace = crate::xdebug::StackTracker::new("", file!(), line!());
    };
    ($msg: literal) => {
        let _stack_trace = crate::xdebug::StackTracker::new($msg, file!(), line!());
    };
}
pub struct StackTracker;

impl StackTracker {
    #[inline(always)]
    pub fn new(msg: &'static str, file: &'static str, line: u32) -> Self {
        if STACK_TRACE {
            unsafe {
                global_xedbug_stack_push(msg.as_ptr(), msg.len(), file.as_ptr(), file.len(), line)
            };
        }
        Self
    }
}

impl Drop for StackTracker {
    #[inline(always)]
    fn drop(&mut self) {
        if STACK_TRACE {
            unsafe { global_xedbug_stack_pop() };
        }
    }
}

extern "C" {
    fn global_xedbug_stack_push(
        msg_ptr: *const u8,
        msg_len: usize,
        file_ptr: *const u8,
        file_len: usize,
        line: u32,
    );
    fn global_xedbug_stack_pop();
}
