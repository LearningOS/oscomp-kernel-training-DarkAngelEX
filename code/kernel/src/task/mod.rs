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
use lazy_static::__Deref;
pub use switch::switch;

use crate::{
    config::{KERNEL_STACK_SIZE, PAGE_SIZE},
    memory::{
        address::{KernelAddr4K, PageCount, UserAddr, UserAddr4K},
        allocator::frame::{self, defualt_allocator, FrameAllocator, FrameTracker},
        StackID, USpaceCreateError, UserSpace,
    },
    riscv::sfence,
    scheduler,
    sync::mutex::SpinLock,
    tools::error::FrameOutOfMemory,
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
    // tcb: Pin<&'static TaskControlBlock>,
    task_status: TaskStatus,   //
    trap_context: TrapContext, // switch to user
    task_context: TaskContext, // switch to scheduler
    parent: Option<Weak<TaskControlBlock>>,
    children: Vec<Arc<TaskControlBlock>>,
    exec_code: i32,
}
impl TaskControlBlockInner {
    pub fn new() -> Self {
        Self {
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
        tcb: *const TaskControlBlock,
        argc: usize,
        argv: usize,
    ) {
        self.task_context.exec_init(kernel_sp, &self.trap_context);
        self.trap_context
            .exec_init(entry_point, user_sp, kernel_sp, tcb, argc, argv);
    }
    pub fn set_tcb_ptr(&mut self, tcb: *const TaskControlBlock) {
        self.trap_context.set_tcb_ptr(tcb);
    }
    pub fn fork_init(&mut self, src: &Self, new_ksp: KernelAddr4K, tcb: *const TaskControlBlock) {
        assert!(src.task_status == TaskStatus::RUNNING);
        self.task_status = TaskStatus::RUNNING;
        self.trap_context = src.trap_context.fork_no_sx(new_ksp, tcb);
        self.task_context = src.task_context.fork(new_ksp, &self.trap_context);
        self.parent = Some(Arc::downgrade(&scheduler::get_current_task()));
        self.children = Vec::new();
        self.exec_code = 0;
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
pub enum TCBCreateError {
    OutOfMemory,
    UserSpace(USpaceCreateError),
}

impl From<FrameOutOfMemory> for TCBCreateError {
    fn from(_: FrameOutOfMemory) -> Self {
        Self::OutOfMemory
    }
}
impl From<USpaceCreateError> for TCBCreateError {
    fn from(e: USpaceCreateError) -> Self {
        match e {
            USpaceCreateError::FrameOutOfMemory(_) => Self::OutOfMemory,
            e @ _ => Self::UserSpace(e),
        }
    }
}

impl TaskControlBlock {
    pub fn new(
        elf_data: &[u8],
        allocator: &mut impl FrameAllocator,
    ) -> Result<Arc<Self>, TCBCreateError> {
        assert!(
            core::mem::size_of::<TaskControlBlock>() < PAGE_SIZE,
            "size of ProcessControlBlock is too large!"
        );
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator).map_err(|e| TCBCreateError::UserSpace(e))?;
        let pid = pid::pid_alloc();
        let thread_group = Arc::new(LockedThreadGroup::new(pid.pid()));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.
        let kernel_stack = allocator.alloc().map_err(|_| TCBCreateError::OutOfMemory)?;
        assert!(KERNEL_STACK_SIZE == PAGE_SIZE);
        let kernel_sp = kernel_stack.ptr().add_one_page();
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
        inner.exec_init(kernel_sp, entry_point, user_sp, tcb.as_ref(), argc, argv);
        Ok(tcb)
    }
    pub fn trap_context_ptr(&self) -> *mut TrapContext {
        &mut self.inner.lock().trap_context
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
    pub fn fork(&self, allocator: &mut impl FrameAllocator) -> Result<Arc<Self>, TCBCreateError> {
        let user_space = self.user_space.lock().fork(allocator)?;
        let kernel_stack = frame::alloc()?;
        let pid_handle = pid::pid_alloc();
        let pid = pid_handle.pid();
        let thread_group = Arc::new(LockedThreadGroup::new(pid));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.
        let stack_id = self.stack_id;
        let kernel_sp = kernel_stack.ptr().add_one_page();
        let new = Arc::new(Self {
            pid: pid_handle,
            tid,
            stack_id,
            thread_group,
            kernel_stack,
            user_space: Arc::new(SpinLock::new(user_space)),
            _no_pin_marker: PhantomPinned,
            inner: SpinLock::new(TaskControlBlockInner::new()),
        });
        let inner = unsafe { new.inner.assert_unique_get() };
        inner.fork_init(&self.inner.lock(), kernel_sp, new.as_ref());
        Ok(new)
    }
    pub fn copy_sx_from(&self, trap_context: &mut TrapContext) -> &Self {
        let inner = unsafe { self.inner.assert_unique_get() };
        inner.trap_context.copy_sx_from(trap_context);
        self
    }
    pub fn set_user_ret(&self, a0: usize) -> &Self {
        let inner = unsafe { self.inner.assert_unique_get() };
        inner.trap_context.set_a0(a0);
        self
    }
    ///
    pub fn exec(
        &self,
        elf_data: &[u8],
        argc: usize,
        argv: usize,
        allocator: &mut impl FrameAllocator,
    ) -> Result<!, USpaceCreateError> {
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator)?;
        // assume only 1 process
        {
            let mut cur_space = self.user_space.lock();
            assert_eq!(cur_space.using_size(), 1);
            unsafe {
                cur_space.clear_user_stack_all(allocator);
            }
            *cur_space = user_space;
            // release lock of user_space
        }
        let ptr = &self.stack_id as *const StackID as *mut StackID;
        unsafe {
            *ptr = stack_id;
        };
        let ncx = {
            let mut inner = self.inner.lock();
            inner.exec_init(
                self.kernel_stack.data(),
                entry_point,
                user_sp,
                self,
                argc,
                argv,
            );
            &mut inner.task_context as *mut TaskContext
        };
        self.using_space();
        sfence::fence_i();
        unsafe { switch::goto_task(ncx) }
    }
}
