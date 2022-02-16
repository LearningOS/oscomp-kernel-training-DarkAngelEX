use core::sync::atomic::Ordering;

use alloc::{sync::Arc, vec::Vec};

use crate::{
    debug::{stack_trace::StackTrace, trace},
    memory::{self},
    riscv::{self, cpu, sfence},
    task::{self, TaskContext, TaskControlBlock},
};

pub mod app;
mod manager;

pub use manager::{add_task, get_initproc};

/// this block can only be access by each hart, no lock is required.
struct Processor {
    current: Option<Arc<TaskControlBlock>>,
    idle_cx: TaskContext,
    add_task_later: bool,
    // debug
    current_stack_trace: *mut StackTrace,
}
impl Processor {
    pub fn idle_cx_ptr(&mut self) -> *mut TaskContext {
        &mut self.idle_cx as *mut TaskContext
    }
}

static mut PROCESSOR: Vec<Processor> = Vec::new();

pub fn init(cpu_count: usize) {
    manager::init();
    for _ in 0..cpu_count {
        unsafe {
            PROCESSOR.push(Processor {
                current: None,
                idle_cx: TaskContext::any(),
                add_task_later: false,
                current_stack_trace: core::ptr::null_mut(),
            });
        }
    }
}
unsafe fn get_processor(hart_id: usize) -> &'static mut Processor {
    &mut PROCESSOR[hart_id]
}
unsafe fn try_get_processor(hart_id: usize) -> Option<&'static mut Processor> {
    PROCESSOR.get_mut(hart_id)
}
fn get_current_processor() -> &'static mut Processor {
    unsafe { get_processor(cpu::hart_id()) }
}
fn get_current_idle_cx_ptr() -> *mut TaskContext {
    unsafe { get_processor(cpu::hart_id()).idle_cx_ptr() }
}

pub fn get_current_task() -> Arc<TaskControlBlock> {
    let p = get_current_processor();
    p.current.as_ref().unwrap().clone()
}
pub fn get_current_task_ptr() -> *const TaskControlBlock {
    let p = get_current_processor();
    p.current.as_ref().unwrap().as_ref()
}
pub fn try_get_current_task_ptr() -> Option<*const TaskControlBlock> {
    let p = unsafe { try_get_processor(cpu::hart_id())? };
    p.current.as_ref().map(|a| a.as_ref() as *const _)
}

pub fn get_current_stack_trace() -> *mut StackTrace {
    unsafe { get_processor(cpu::hart_id()).current_stack_trace }
}
pub fn try_get_current_stack_trace() -> Option<*mut StackTrace> {
    unsafe { try_get_processor(cpu::hart_id()).map(|a| a.current_stack_trace) }
}
fn set_current_stack_trace(ptr: *mut StackTrace) {
    unsafe { get_processor(cpu::hart_id()).current_stack_trace = ptr }
}
fn clear_current_stack_trace() {
    unsafe { get_processor(cpu::hart_id()).current_stack_trace = core::ptr::null_mut() }
}

pub fn add_task_later() {
    let f = &mut get_current_processor().add_task_later;
    debug_check!(!*f);
    *f = true;
}

pub fn run_task(hart_id: usize) -> ! {
    let processor = unsafe { get_processor(hart_id) };
    let idle_cx_ptr = processor.idle_cx_ptr();
    // println!("hart {} sp: {:#x}", hart_id, riscv::current_sp());
    // trace::print_sp();
    loop {
        let task = match manager::fetch_task() {
            Some(task) => task,
            None => continue,
        };
        let next_cx = task.task_context_ptr_scheduler();
        task.using_space_scheduler();
        sfence::fence_i();
        // let pid = task.pid();
        // println!("hart {} run task {:?}", hart_id, pid);
        // release prev task there.
        set_current_stack_trace(&task.stack_trace as *const _ as *mut _);
        processor.current = Some(task);
        unsafe {
            let _ = task::switch(idle_cx_ptr, next_cx);
        }
        memory::set_satp_by_global();
        clear_current_stack_trace();
        // println!("hart {} exit task {:?}", hart_id, pid);
        if processor.add_task_later {
            manager::add_task(processor.current.take().unwrap());
            processor.add_task_later = false;
        } else {
            processor.current = None; // release
        }
    }
}
