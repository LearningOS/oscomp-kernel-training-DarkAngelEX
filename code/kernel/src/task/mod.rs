mod context;
mod pid;
mod switch;
mod thread;
mod tid;

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};
pub use switch::switch;

use crate::{
    config::{KERNEL_STACK_SIZE, PAGE_SIZE},
    memory::{
        allocator::frame::{self, defualt_allocator, FrameAllocator, FrameTracker},
        StackID, UserSpace, UserSpaceCreateError,
    },
    sync::mutex::SpinLock,
    trap::context::TrapContext,
};

use self::{context::TaskContext, pid::PidHandle, thread::ThreadGroup, tid::Tid};

#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    RUNNING,         // 可被调度
    INTERRUPTIBLE,   // 可中断睡眠
    UNINTERRUPTIBLE, // 不可中断睡眠 等待资源
    STOPPED,         // 被暂停
    ZOMBIE,          // 等待回收
    DEAD,            // 等待销毁
}

pub struct TaskControlBlock {
    pid: PidHandle,
    tid: Tid,
    stack_id: StackID,
    thread_group: Arc<ThreadGroup>,
    kernel_stack: FrameTracker,
    user_space: Arc<SpinLock<UserSpace>>, // share with thread group
    task_status: TaskStatus,              //
    trap_context: TrapContext,            // switch to user
    task_context: TaskContext,            // switch to scheduler
    parent: Option<Weak<TaskControlBlock>>,
    children: Vec<Arc<TaskControlBlock>>,
    exec_code: i32,
}

impl Drop for TaskControlBlock {
    fn drop(&mut self) {
        unsafe {
            let allocator = &mut defualt_allocator();
            let mut space = self.user_space.lock();
            space.stack_dealloc(self.stack_id, allocator);
            self.thread_group.dealloc(self.tid);
        }
    }
}

pub enum CreateError {
    OutOfMemory,
    UserSpace(UserSpaceCreateError),
}

impl TaskControlBlock {
    pub fn new(
        elf_data: &[u8],
        allocator: &mut impl FrameAllocator,
    ) -> Result<Box<Self>, CreateError> {
        assert!(
            core::mem::size_of::<TaskControlBlock>() < PAGE_SIZE,
            "size of ProcessControlBlock is too large!"
        );
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator).map_err(|e| CreateError::UserSpace(e))?;
        let pid = pid::pid_alloc();
        let thread_group = Arc::new(ThreadGroup::new(pid.pid()));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.
        let kernel_stack = frame::alloc().map_err(|_| CreateError::OutOfMemory)?;
        assert!(KERNEL_STACK_SIZE == PAGE_SIZE);
        let kernel_stack_ptr = kernel_stack.ptr().add_n_pg(1);
        let mut pcb = Box::new(Self {
            pid,
            tid,
            stack_id,
            thread_group,
            kernel_stack,
            user_space: Arc::new(SpinLock::new(user_space)),
            task_status: TaskStatus::RUNNING,
            trap_context: unsafe { TrapContext::any() },
            task_context: unsafe { TaskContext::any() },
            parent: None,
            children: Vec::new(),
            exec_code: 0,
        });
        pcb.task_context = TaskContext::goto_trap_return(kernel_stack_ptr, &pcb.trap_context);
        pcb.trap_context = TrapContext::app_init(entry_point, user_sp, kernel_stack_ptr);
        Ok(pcb)
    }
}
