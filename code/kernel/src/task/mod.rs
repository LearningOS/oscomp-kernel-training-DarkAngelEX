mod context;
mod pid;
mod switch;
mod thread;
mod tid;

use core::{
    marker::PhantomPinned,
    sync::atomic::{AtomicI32, Ordering},
};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};
pub use switch::{goto_task, switch};

use crate::{
    config::PAGE_SIZE,
    memory::{
        address::{KernelAddr4K, UserAddr, UserAddr4K},
        allocator::frame::{self, FrameAllocator},
        stack::{self, KernelStackTracker},
        StackID, USpaceCreateError, UserSpace,
    },
    riscv::sfence,
    scheduler::{self, get_current_task, get_initproc},
    sync::mutex::{MutexGuard, Spin, SpinLock},
    tools::error::{FrameOutOfMemory, HeapOutOfMemory},
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
    pid: PidHandle,
    parent: SpinLock<Option<Weak<TaskControlBlock>>>,
    task_status: SpinLock<TaskStatus>, //
    alive: Option<Box<AliveTaskControlBlock>>,
    exit_code: AtomicI32,
}

pub struct AliveTaskControlBlock {
    // immutable
    tid: Tid,
    stack_id: StackID,
    thread_group: Arc<LockedThreadGroup>,
    kernel_stack: KernelStackTracker,
    user_space: Arc<SpinLock<UserSpace>>, // share with thread group
    _no_pin_marker: PhantomPinned,
    // mutable
    inner: SpinLock<TaskControlBlockInner>,
}

pub struct TaskControlBlockInner {
    // tcb: Pin<&'static TaskControlBlock>,
    trap_context: TrapContext, // switch to user
    task_context: TaskContext, // switch to scheduler
    children: Vec<Arc<TaskControlBlock>>,
}
impl TaskControlBlockInner {
    pub fn new() -> Self {
        Self {
            trap_context: unsafe { TrapContext::any() },
            task_context: unsafe { TaskContext::any() },
            children: Vec::new(),
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
        self.trap_context = src.trap_context.fork_no_sx(new_ksp, tcb);
        self.task_context = src.task_context.fork(new_ksp, &self.trap_context);
        self.children = Vec::new();
    }
    pub fn get_children(&mut self) -> &mut Vec<Arc<TaskControlBlock>> {
        &mut self.children
    }
}

impl Drop for AliveTaskControlBlock {
    fn drop(&mut self) {
        unsafe {
            memory_trace!("TaskControlBlock::stack_alloc begin");
            let allocator = &mut frame::defualt_allocator();
            let mut space = self.user_space.lock();
            space.stack_dealloc(self.stack_id, allocator);
            self.thread_group.dealloc(self.tid);
            memory_trace!("TaskControlBlock::stack_alloc end");
        }
    }
}

#[derive(Debug)]
pub enum TCBCreateError {
    OutOfMemory,
    UserSpace(USpaceCreateError),
}
impl From<HeapOutOfMemory> for TCBCreateError {
    fn from(_: HeapOutOfMemory) -> Self {
        Self::OutOfMemory
    }
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
        let kernel_stack = stack::alloc_kernel_stack()?;
        let kernel_sp = kernel_stack.bottom();
        let tcb = Arc::new(Self {
            pid,
            task_status: SpinLock::new(TaskStatus::RUNNING),
            parent: SpinLock::new(None),
            exit_code: AtomicI32::new(0),
            alive: Some(Box::new(AliveTaskControlBlock {
                tid,
                stack_id,
                thread_group,
                kernel_stack,
                user_space: Arc::new(SpinLock::new(user_space)),
                _no_pin_marker: PhantomPinned,
                inner: SpinLock::new(TaskControlBlockInner::new()),
            })),
        });
        let (argc, argv) = (0, 0);
        let inner = unsafe { tcb.alive().inner.assert_unique_get() };
        inner.exec_init(kernel_sp, entry_point, user_sp, tcb.as_ref(), argc, argv);
        Ok(tcb)
    }
    pub fn try_alive(&self) -> Option<&AliveTaskControlBlock> {
        self.alive.as_ref().map(|a| a.as_ref())
    }
    pub fn alive(&self) -> &AliveTaskControlBlock {
        debug_run! {{
            let flag = self.task_status.lock().clone();
            assert!(flag != TaskStatus::ZOMBIE && flag != TaskStatus::DEAD);
        }};
        self.alive.as_ref().unwrap()
    }
    pub fn lock(&self) -> MutexGuard<TaskControlBlockInner, Spin> {
        self.alive().inner.lock()
    }
    pub fn trap_context_ptr(&self) -> *mut TrapContext {
        unsafe { &mut (*self.alive().inner.get_ptr()).trap_context }
    }
    pub fn task_context_ptr(&self) -> *mut TaskContext {
        unsafe { &mut (*self.alive().inner.get_ptr()).task_context }
    }
    pub fn using_space(&self) {
        self.alive().user_space.lock().using();
    }
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }
    pub fn is_zombie(&self) -> bool {
        *self.task_status.lock() == TaskStatus::ZOMBIE
    }
    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::Acquire)
    }
    pub fn set_parent(&self, parent: &Arc<Self>) {
        let weak = Some(Arc::downgrade(parent));
        *self.parent.lock() = weak;
    }
    pub fn kernel_bottom(&self) -> KernelAddr4K {
        self.alive().kernel_stack.bottom()
    }
    pub fn try_kernel_bottom(&self) -> Option<KernelAddr4K> {
        self.try_alive().map(|x| x.kernel_stack.bottom())
    }
    pub fn fork(&self, allocator: &mut impl FrameAllocator) -> Result<Arc<Self>, TCBCreateError> {
        memory_trace!("TaskControlBlock::fork");
        let alive = self.alive();
        let user_space = alive.user_space.lock().fork(allocator)?;
        let kernel_stack = stack::alloc_kernel_stack()?;
        let pid_handle = pid::pid_alloc();
        let pid = pid_handle.pid();
        let thread_group = Arc::new(LockedThreadGroup::new(pid));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.
        let stack_id = alive.stack_id;
        let kernel_sp = kernel_stack.bottom();
        let new = Arc::new(Self {
            pid: pid_handle,
            task_status: SpinLock::new(TaskStatus::RUNNING),
            parent: SpinLock::new(Some(Arc::downgrade(&get_current_task()))),
            exit_code: AtomicI32::new(0),
            alive: Some(Box::new(AliveTaskControlBlock {
                tid,
                stack_id,
                thread_group,
                kernel_stack,
                user_space: Arc::new(SpinLock::new(user_space)),
                _no_pin_marker: PhantomPinned,
                inner: SpinLock::new(TaskControlBlockInner::new()),
            })),
        });
        let new_alive = new.alive();
        let inner = unsafe { new_alive.inner.assert_unique_get() };
        inner.fork_init(&alive.inner.lock(), kernel_sp, new.as_ref());
        Ok(new)
    }
    pub fn copy_sx_from(&self, trap_context: &mut TrapContext) -> &Self {
        let inner = unsafe { self.alive().inner.assert_unique_get() };
        inner.trap_context.copy_sx_from(trap_context);
        self
    }
    pub fn set_user_ret(&self, a0: usize) -> &Self {
        let inner = unsafe { self.alive().inner.assert_unique_get() };
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
        memory_trace!("TaskControlBlock::exec 0");
        let alive = self.alive();
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator)?;
        // assume only 1 process
        {
            let mut cur_space = alive.user_space.lock();
            assert_eq!(cur_space.using_size(), 1);
            unsafe {
                cur_space.clear_user_stack_all(allocator);
            }
            *cur_space = user_space;
            // release lock of user_space
        }
        let ptr = &alive.stack_id as *const StackID as *mut StackID;
        unsafe {
            *ptr = stack_id;
        };
        memory_trace!("TaskControlBlock::exec 1");
        let kernel_sp = alive.kernel_stack.bottom();
        let ncx = {
            let mut inner = alive.inner.lock();
            inner.exec_init(kernel_sp, entry_point, user_sp, self, argc, argv);
            &mut inner.task_context as *mut TaskContext
        };
        self.using_space();
        memory_trace!("TaskControlBlock::exec 2");
        sfence::fence_i();
        unsafe { switch::goto_task(ncx) }
    }
    pub fn exit(&self, exit_code: i32) {
        memory_trace!("TaskControlBlock::exit entry");
        self.exit_code.store(exit_code, Ordering::Relaxed);
        {
            let mut children = core::mem::take(&mut self.alive().inner.lock().children);
            let initproc = get_initproc();
            let inner = &mut initproc.alive().inner.lock().children;
            while let Some(child) = children.pop() {
                child.set_parent(&initproc);
                inner.push(child);
            }
        }
        *self.task_status.lock() = TaskStatus::ZOMBIE;
        let x = &self.alive as *const _ as *mut Option<Box<AliveTaskControlBlock>>;
        unsafe { *x = None };
        memory_trace!("TaskControlBlock::exit return");
    }
}
