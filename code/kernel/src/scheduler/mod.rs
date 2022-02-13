use alloc::{sync::Arc, vec::Vec};

use crate::{
    memory::{self},
    riscv::{cpu, sfence},
    task::{self, TaskContext, TaskControlBlock}, debug::trace,
};

pub mod app;
mod manager;

pub use manager::{add_task, get_initproc};

struct Processor {
    current: Option<Arc<TaskControlBlock>>,
    idle_cx: TaskContext,
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
            });
        }
    }
}
fn get_processor(hart_id: usize) -> &'static mut Processor {
    unsafe { &mut PROCESSOR[hart_id] }
}
fn try_get_processor(hart_id: usize) -> Option<&'static mut Processor> {
    unsafe { PROCESSOR.get_mut(hart_id) }
}
fn get_current_processor() -> &'static mut Processor {
    get_processor(cpu::hart_id())
}
fn get_current_idle_cx_ptr() -> *mut TaskContext {
    get_processor(cpu::hart_id()).idle_cx_ptr()
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
    let p = try_get_processor(cpu::hart_id())?;
    p.current.as_ref().map(|a| a.as_ref() as *const _)
}

pub fn run_task(hart_id: usize) -> ! {
    let processor = get_processor(hart_id);
    let idle_cx_ptr = processor.idle_cx_ptr();
    // trace::print_sp();
    loop {
        let task = match manager::fetch_task() {
            Some(task) => task,
            None => continue,
        };
        let next_cx = task.task_context_ptr();
        task.using_space();
        // let pid = task.pid();
        // println!("hart {} run task {:?}", hart_id, pid);
        // release prev task there.
        processor.current = Some(task);
        // sfence::fence_i();
        unsafe {
            let _ = task::switch(idle_cx_ptr, next_cx);
        }
        memory::set_satp_by_global();
        // println!("hart {} exit task {:?}", hart_id, pid);
        processor.current = None; // release
    }
}
