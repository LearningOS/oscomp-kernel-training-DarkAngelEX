mod context;
mod pid;
mod switch;
mod thread;
mod tid;

use core::{marker::PhantomPinned, pin::Pin};

use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};
pub use switch::switch;

use crate::{
    config::{KERNEL_STACK_SIZE, PAGE_SIZE},
    memory::{
        address::{KernelAddr4K, UserAddr, UserAddr4K},
        allocator::frame::{self, defualt_allocator, FrameAllocator, FrameTracker},
        StackID, UserSpace, UserSpaceCreateError,
    },
    sync::mutex::SpinLock,
    trap::context::TrapContext,
};

use self::thread::LockedThreadGroup;
pub use self::{
    context::TaskContext,
    pid::{Pid, PidHandle},
    tid::Tid,
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum TaskStatus {
    RUNNING,         // 可被调度
    INTERRUPTIBLE,   // 可中断睡眠
    UNINTERRUPTIBLE, // 不可中断睡眠 等待资源
    STOPPED,         // 被暂停
    ZOMBIE,          // 等待回收
    DEAD,            // 等待销毁
}

pub struct TaskControlBlock {
    // immutable
    pid: PidHandle,
    tid: Tid,
    stack_id: StackID,
    thread_group: Arc<LockedThreadGroup>,
    kernel_stack: FrameTracker,
    user_space: Arc<SpinLock<UserSpace>>, // share with thread group
    _no_pin_marker: PhantomPinned,
    // mutable
    inner: SpinLock<TaskControlBlockInner>,
}

pub struct TaskControlBlockInner {
    tcb: Pin<&'static TaskControlBlock>,
    task_status: TaskStatus,   //
    trap_context: TrapContext, // switch to user
    task_context: TaskContext, // switch to scheduler
    parent: Option<Weak<TaskControlBlock>>,
    children: Vec<Arc<TaskControlBlock>>,
    exec_code: i32,
}
impl TaskControlBlockInner {
    pub fn new() -> Self {
        #[allow(deref_nullptr)]
        let null = unsafe { &*core::ptr::null() };
        Self {
            tcb: unsafe { Pin::new_unchecked(null) },
            task_status: TaskStatus::RUNNING,
            trap_context: unsafe { TrapContext::any() },
            task_context: unsafe { TaskContext::any() },
            parent: None,
            children: Vec::new(),
            exec_code: 0,
        }
    }
    pub fn exec_init(
        &mut self,
        kernel_sp: KernelAddr4K,
        entry_point: UserAddr,
        user_sp: UserAddr4K,
        argc: usize,
        argv: usize,
        tcb: *const TaskControlBlock,
    ) {
        self.task_context.exec_init(kernel_sp, &self.trap_context);
        self.trap_context
            .exec_init(entry_point, user_sp, kernel_sp, argc, argv);
        self.set_tcb_ptr(tcb);
    }
    pub fn set_tcb_ptr(&mut self, tcb: *const TaskControlBlock) {
        self.trap_context.set_tcb_ptr(tcb);
        self.tcb = unsafe { Pin::new_unchecked(&*tcb) }
    }
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

#[derive(Debug)]
pub enum CreateError {
    OutOfMemory,
    UserSpace(UserSpaceCreateError),
}

impl TaskControlBlock {
    pub fn new(
        elf_data: &[u8],
        allocator: &mut impl FrameAllocator,
    ) -> Result<Arc<Self>, CreateError> {
        assert!(
            core::mem::size_of::<TaskControlBlock>() < PAGE_SIZE,
            "size of ProcessControlBlock is too large!"
        );
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator).map_err(|e| CreateError::UserSpace(e))?;
        let pid = pid::pid_alloc();
        let thread_group = Arc::new(LockedThreadGroup::new(pid.pid()));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.
        let kernel_stack = frame::alloc().map_err(|_| CreateError::OutOfMemory)?;
        assert!(KERNEL_STACK_SIZE == PAGE_SIZE);
        let kernel_sp = kernel_stack.ptr().add_n_pg(1);
        let tcb = Arc::new(Self {
            pid,
            tid,
            stack_id,
            thread_group,
            kernel_stack,
            user_space: Arc::new(SpinLock::new(user_space)),
            _no_pin_marker: PhantomPinned,
            inner: SpinLock::new(TaskControlBlockInner::new()),
        });
        let (argc, argv) = (0, 0);
        let inner = unsafe { tcb.inner.assert_unique_get() };
        inner.exec_init(kernel_sp, entry_point, user_sp, argc, argv, tcb.as_ref());

        Ok(tcb)
    }
    pub fn task_context_ptr(&self) -> *mut TaskContext {
        &mut self.inner.lock().task_context
    }
    pub fn using_space(&self) {
        self.user_space.lock().using();
    }
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }
}
