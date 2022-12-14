use core::sync::atomic::{AtomicUsize, Ordering};

use crate::{config::KERNEL_STACK_SIZE, hart, local, tools};

use super::OPEN_DEBUG;

pub const OPEN_MEMORY_TRACE: bool = false;
pub const STACK_DETECTION: bool = true && OPEN_DEBUG;
pub const TRACE_ADDR: usize = 0xfffffff080390010;

#[inline(never)]
#[allow(unreachable_code, unused_variables)]
pub fn trace_by_cnt(cnt: usize) -> bool {
    return false;
    // return false;
    if cnt >= 500 {
        return true;
    }
    // let set = [510];
    // for i in set {
    //     if cnt == i {
    //         return true;
    //     }
    // }
    false
}

#[macro_export]
macro_rules! memory_trace {
    () => {
        if $crate::xdebug::trace::OPEN_MEMORY_TRACE {
            $crate::xdebug::trace::memory_trace("", file!(), line!());
        }
        if $crate::xdebug::trace::STACK_DETECTION {
            $crate::xdebug::trace::stack_detection();
        }
    };
    ($name: expr) => {
        if $crate::xdebug::trace::OPEN_MEMORY_TRACE {
            $crate::xdebug::trace::memory_trace($name, file!(), line!());
        }
        if $crate::xdebug::trace::STACK_DETECTION {
            $crate::xdebug::trace::stack_detection();
        }
    };
}

#[macro_export]
macro_rules! memory_trace_show {
    ($name: expr) => {
        if $crate::xdebug::trace::OPEN_MEMORY_TRACE {
            $crate::xdebug::trace::memory_trace_show($name, file!(), line!());
        }
        if $crate::xdebug::trace::STACK_DETECTION {
            $crate::xdebug::trace::stack_detection();
        }
    };
}

static PREV_VALUE: AtomicUsize = AtomicUsize::new(0);

static TRACE_CNT: AtomicUsize = AtomicUsize::new(0);

pub fn prev_value() -> usize {
    PREV_VALUE.load(Ordering::Acquire)
}
pub fn current_value() -> usize {
    let ptr = TRACE_ADDR as *const usize;
    unsafe { ptr.read_volatile() }
}
pub fn current_update() -> (usize, usize, bool) {
    let prev = prev_value();
    let current = current_value();
    let change = prev != current;
    if change {
        PREV_VALUE.store(current, Ordering::Release);
    }
    (prev, current, change)
}
pub fn current_count() -> usize {
    TRACE_CNT.load(Ordering::Acquire)
}

#[inline(never)]
pub fn memory_trace(name: &'static str, file: &'static str, line: u32) {
    let cnt = TRACE_CNT.fetch_add(1, Ordering::SeqCst);
    let (prev, current, change) = current_update();
    if change {
        PREV_VALUE.store(current, Ordering::Release);
        println!(
            "\x1b[32mvalue change\x1b[0m {:#016x} -> {:#016x} count: {} in {}, {}:{}",
            prev, current, cnt, name, file, line
        );
    } else if trace_by_cnt(cnt) {
        println!(
            "trace value {:#016x} count: {},  in {}, {}:{}",
            current, cnt, name, file, line
        );
    }
    stack_detection();
}

#[inline(never)]
pub fn memory_trace_show(name: &'static str, file: &'static str, line: u32) {
    let current = current_value();
    let cnt = current_count();
    println!(
        "trace value {:#016x} count: {},  in {}, {}:{}",
        current, cnt, name, file, line
    );
}

#[inline(never)]
pub fn call_when_alloc() {
    let cnt = current_count();
    println!(
        "\x1b[32m!!!!! alloc\x1b[0m {:#016x} count: {} current sp: {:#016x}",
        TRACE_ADDR,
        cnt,
        hart::current_sp()
    );
    let ptr = TRACE_ADDR as *const usize;
    PREV_VALUE.store(unsafe { ptr.read_volatile() }, Ordering::Release);
}

#[inline(never)]
pub fn call_when_dealloc() {
    println!("\x1b[32m!!!!! dealloc\x1b[0m {:#016x}", TRACE_ADDR);
}

pub fn print_sp() {
    let sp = hart::current_sp();
    println!("current sp {:#016x}", sp);
}

fn using_stack_size_print_impl(current: usize) {
    let (m, k, b) = tools::size_to_mkb(current);
    print!("stack size: ");
    if m > 0 {
        print!("{m}M {k}K {b}Bytes");
    } else if k > 0 {
        print!("{k}K {b}Bytes");
    } else if b > 0 {
        print!("{b}Bytes");
    } else {
        print!("null");
    }
    // print!(" sp: {:#x}", hart::current_sp());
    // print!(" hart: {}", cpu::hart_id());
    if current >= KERNEL_STACK_SIZE {
        print!(" \x1b[31m!!!stack over flow!!!\x1b[0m");
    }
    println!();
}

/// ?????????????????????. ???????????????????????????????????????????????????
pub fn using_stack_size() -> usize {
    let current = local::stack_size();
    if current >= KERNEL_STACK_SIZE {
        using_stack_size_print_impl(current);
    }
    current
}

pub fn using_stack_size_print() -> usize {
    let current = local::stack_size();
    using_stack_size_print_impl(current);
    current
}

#[inline(always)]
pub fn stack_detection() {
    using_stack_size();
}
