use core::sync::atomic::Ordering;

use alloc::{sync::Arc, vec::Vec};

use crate::{
    debug::trace,
    memory::{self},
    riscv::{cpu, sfence},
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

pub fn add_task_later() {
    let f = &mut get_current_processor().add_task_later;
    debug_check!(!*f);
    *f = true;
}

pub fn run_task(hart_id: usize) -> ! {
    let processor = unsafe { get_processor(hart_id) };
    let idle_cx_ptr = processor.idle_cx_ptr();
    // trace::print_sp();
    loop {
        let task = match manager::fetch_task() {
            Some(task) => task,
            None => continue,
        };
        let next_cx = task.task_context_ptr_scheduler();
        task.using_space_scheduler();
        sfence::fence_i();
        // sfence::sfence_vma_all_no_global();
        // let pid = task.pid();
        // println!("hart {} run task {:?}", hart_id, pid);
        // release prev task there.
        processor.current = Some(task);
        // core::sync::atomic::fence(Ordering::SeqCst);
        unsafe {
            let _ = task::switch(idle_cx_ptr, next_cx);
        }
        // core::sync::atomic::fence(Ordering::SeqCst);
        memory::set_satp_by_global();
        // println!("hart {} exit task {:?}", hart_id, pid);
        if processor.add_task_later {
            manager::add_task(processor.current.take().unwrap());
            processor.add_task_later = false;
        } else {
            processor.current = None; // release
        }
    }
}
