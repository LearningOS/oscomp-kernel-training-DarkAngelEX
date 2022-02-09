use alloc::{collections::VecDeque, sync::Arc};

use crate::{loader, memory::allocator::frame, sync::mutex::SpinLock, task::TaskControlBlock};

type PTCB = Arc<TaskControlBlock>;
struct ReadyManager {
    ready_queue: Option<VecDeque<PTCB>>,
    initproc: Option<PTCB>,
}

impl ReadyManager {
    pub const fn new() -> Self {
        Self {
            ready_queue: None,
            initproc: None,
        }
    }
    pub fn init(&mut self) {
        assert!(self.ready_queue.is_none() && self.initproc.is_none());
        self.ready_queue = Some(VecDeque::new());
        let allocator = &mut frame::defualt_allocator();
        self.initproc = Some(
            TaskControlBlock::new(loader::get_app_data_by_name("initproc").unwrap(), allocator)
                .unwrap(),
        );
        // let initproc into scheduler
        self.add(self.initproc().clone());
        println!("[FTL OS]initproc has been loaded into the scheduler");
    }
    fn get_queue(&mut self) -> &mut VecDeque<PTCB> {
        debug_check!(!self.ready_queue.is_none(), "ready_queue no init");
        unsafe { self.ready_queue.as_mut().unwrap_unchecked() }
    }
    fn initproc(&self) -> &PTCB {
        debug_check!(!self.initproc.is_none(), "initproc no init");
        unsafe { self.initproc.as_ref().unwrap_unchecked() }
    }
    fn add(&mut self, task: PTCB) {
        self.get_queue().push_back(task);
    }
    fn fetch(&mut self) -> Option<PTCB> {
        self.get_queue().pop_front()
    }
}

static READY_MANAGER: SpinLock<ReadyManager> = SpinLock::new(ReadyManager::new());

pub fn init() {
    println!("[FTL OS]scheduler manager init");
    READY_MANAGER.lock().init();
}
pub fn add_task(task: PTCB) {
    READY_MANAGER.lock().add(task);
}
pub fn fetch_task() -> Option<PTCB> {
    READY_MANAGER.lock().fetch()
}
