use alloc::{sync::Arc, vec::Vec};

use crate::{
    memory::{self, stack::KernelStackTracker},
    riscv::{self, cpu, sfence},
    task::{self, TaskContext, TaskControlBlock},
    timer::{self, TimeTicks},
    xdebug::{stack_trace::StackTrace, trace},
};

pub mod app;
mod manager;

pub use manager::{add_task, add_task_group, get_initproc};

enum TaskGoto {
    None,
    Scheduler,
    Timer(TimeTicks), // target tick
}
impl Default for TaskGoto {
    fn default() -> Self {
        Self::None
    }
}
impl TaskGoto {
    pub fn take(&mut self) -> Self {
        core::mem::take(self)
    }
}

/// this block can only be access by each hart, no lock is required.
struct Processor {
    current: Option<Arc<TaskControlBlock>>,
    idle_cx: TaskContext,
    task_goto: TaskGoto,
    kernel_stack_save: Option<KernelStackTracker>, // delay free kernel stack
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
                task_goto: TaskGoto::None,
                kernel_stack_save: None,
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

pub fn free_kernel_stack_later(kernel_stack: KernelStackTracker) {
    get_current_processor().kernel_stack_save = Some(kernel_stack);
}
pub fn get_current_kernel_stack_save() -> Option<&'static KernelStackTracker> {
    get_current_processor().kernel_stack_save.as_ref()
}

pub fn add_task_later() {
    let goto = &mut get_current_processor().task_goto;
    debug_check!(matches!(goto, TaskGoto::None));
    *goto = TaskGoto::Scheduler;
}

pub fn add_timer_later(target_tick: TimeTicks) {
    let goto = &mut get_current_processor().task_goto;
    debug_check!(matches!(goto, TaskGoto::None));
    *goto = TaskGoto::Timer(target_tick);
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
        let current_task = processor.current.take().unwrap();
        // println!("hart {} exit task {:?}", hart_id, pid);
        match processor.task_goto.take() {
            TaskGoto::None => (),
            TaskGoto::Scheduler => manager::add_task(current_task),
            TaskGoto::Timer(target_tick) => {
                timer::sleep::timer_push_task(target_tick, current_task)
            }
        }
    }
}
