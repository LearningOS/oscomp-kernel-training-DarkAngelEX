use alloc::{sync::Arc, vec::Vec};

use crate::{
    riscv::{cpu, sfence},
    task::{self, TaskContext, TaskControlBlock}, memory::{set_satp_by_global, self},
};

mod manager;

pub use manager::add_task;

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

pub fn get_current_task() -> Arc<TaskControlBlock> {
    let p = get_processor(cpu::hart_id());
    p.current.as_ref().unwrap().clone()
}

pub fn run_task(hart_id: usize) -> ! {
    let processor = get_processor(hart_id);
    let idle_cx_ptr = processor.idle_cx_ptr();
    loop {
        let task = match manager::fetch_task() {
            Some(task) => task,
            None => continue,
        };
        let next_cx = task.task_context_ptr();
        task.using_space();
        println!("hart {} run task {:?}", hart_id, task.pid());
        // release prev task there.
        processor.current = Some(task);
        sfence::fence_i();
        unsafe {
            let _ = task::switch(idle_cx_ptr, next_cx);
        }
        memory::set_satp_by_global();
    }
}
