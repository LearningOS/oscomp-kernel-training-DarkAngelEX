#![allow(dead_code)]
// 评测机使用
#[cfg(feature = "submit")]
pub const OPEN_DEBUG: bool = false;
// 本地测试使用
#[cfg(not(feature = "submit"))]
pub const OPEN_DEBUG: bool = false;

pub const PRINT_MAP_ALL: bool = false;

// pub const CLOSE_ALL_DEBUG: bool = true;

use riscv::register::sstatus;

pub const PRINT_SYSCALL_ALL: bool = false;
pub const PRINT_SYSCALL: bool = false;
pub const PRINT_SYSCALL_RW: bool = false; // 输出 read 和 write 系统调用
pub const PRINT_SYSCALL_ERR: bool = false; // 当系统调用返回Err时输出, 只在PRINT_SYSCALL_ALL=false时有效
pub const PRINT_FS_OPEN_PATH: bool = false; // 输出所有对VFS的访问路径
pub const PRINT_DROP_TCB: bool = false; // check drop when becomes zombie
pub const PRINT_PAGE_FAULT: bool = false;
pub const PRINT_HANDLE_SIGNAL: bool = false;
pub const PRINT_TICK: bool = false;

pub const PRINT_ABNORMALLY_EXIT: bool = false; // thread Pid(x) Tid(y) terminal abnormally

pub const CLOSE_FRAME_DEALLOC: bool = false;
pub const CLOSE_HEAP_DEALLOC: bool = false;
pub const CLOSE_LOCAL_HEAP: bool = false;

pub const FRAME_DEALLOC_OVERWRITE: bool = (true || FRAME_MODIFY_CHECK) && OPEN_DEBUG;
pub const HEAP_DEALLOC_OVERWRITE: bool = true && OPEN_DEBUG;
pub const HEAP_ALLOC_OVERWRITE: bool = true && OPEN_DEBUG;

pub const FRAME_RELEASE_CHECK: bool = true && OPEN_DEBUG; // 检测frame是否被二次释放
pub const FRAME_MODIFY_CHECK: bool = true && OPEN_DEBUG; // 检测frame释放后是否被修改
pub const HEAP_RELEASE_CHECK: bool = false && OPEN_DEBUG;
pub const HEAP_PROTECT: bool = false && OPEN_DEBUG;

pub const CLOSE_TIME_INTERRUPT: bool = false && OPEN_DEBUG;

pub const NO_SYSCALL_PANIC: bool = false && OPEN_DEBUG;

pub const CLOSE_RANDOM: bool = true; // 让每次系统运行结果都一样, 不使用基于时钟的随机

pub const LIMIT_SIGNAL_COUNT: Option<usize> = None; // 信号处理超过预定数量时panic
pub const CRITICAL_END_FORCE: bool = (false || CLOSE_TIME_INTERRUPT) && OPEN_DEBUG;

#[macro_use]
pub mod trace;
#[macro_use]
pub mod stack_trace;

pub fn init() {
    stack_trace::init();
    ftl_util::xdebug::sie_init(|| sstatus::read().sie());
}

#[allow(unused_macros)]
macro_rules! place {
    () => {
        concat!(file!(), ":", line!(), ":", column!())
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
