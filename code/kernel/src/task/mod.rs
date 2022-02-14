mod context;
mod pid;
mod switch;
mod thread;
mod tid;

use core::{
    marker::PhantomPinned,
    sync::atomic::{AtomicI32, AtomicUsize, Ordering},
};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};
pub use switch::{goto_task, switch};

use crate::{
    config::PAGE_SIZE,
    debug::PRINT_DROP_TCB,
    memory::{
        address::{KernelAddr4K, UserAddr, UserAddr4K},
        allocator::{
            self,
            frame::{self, FrameAllocator},
        },
        stack::{self, KernelStackTracker},
        StackID, USpaceCreateError, UserSpace,
    },
    riscv::{self, cpu, sfence},
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

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TaskStatus {
    RUNNING = 0,         // 可被调度
    INTERRUPTIBLE = 1,   // 可中断睡眠
    UNINTERRUPTIBLE = 2, // 不可中断睡眠 等待资源
    STOPPED = 3,         // 被暂停
    ZOMBIE = 4,          // 等待回收
    DEAD = 5,            // 等待销毁
}
impl From<usize> for TaskStatus {
    fn from(v: usize) -> Self {
        match v {
            0 => TaskStatus::RUNNING,
            1 => TaskStatus::INTERRUPTIBLE,
            2 => TaskStatus::UNINTERRUPTIBLE,
            3 => TaskStatus::STOPPED,
            4 => TaskStatus::ZOMBIE,
            5 => TaskStatus::DEAD,
            _ => panic!(),
        }
    }
}

pub struct AtomicTaskStatus(AtomicUsize);

impl From<TaskStatus> for AtomicTaskStatus {
    fn from(ts: TaskStatus) -> Self {
        Self(AtomicUsize::new(ts as usize))
    }
}
impl From<AtomicTaskStatus> for TaskStatus {
    fn from(ts: AtomicTaskStatus) -> Self {
        ts.0.load(Ordering::Acquire).into()
    }
}
impl AtomicTaskStatus {
    pub fn load(&self) -> TaskStatus {
        self.0.load(Ordering::Acquire).into()
    }
    pub fn store(&self, val: TaskStatus) {
        self.0.store(val as usize, Ordering::Release)
    }
    pub fn load_relax(&self) -> TaskStatus {
        self.0.load(Ordering::Relaxed).into()
    }
}

pub struct TaskControlBlock {
    pid: PidHandle,
    parent: SpinLock<Option<Weak<TaskControlBlock>>>,
    pub task_status: AtomicTaskStatus,
    alive: Option<Box<AliveTaskControlBlock>>,
    exit_code: AtomicI32,
}
impl Drop for TaskControlBlock {
    fn drop(&mut self) {
        if PRINT_DROP_TCB {
            println!("TCB drop! pid: {:?}", self.pid());
        }
    }
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
    /// self will become subprocess
    fn fork_init(&mut self, src: &Self, new_ksp: KernelAddr4K, tcb: *const TaskControlBlock) {
        self.trap_context = src.trap_context.fork_no_sx(new_ksp, tcb);
        self.task_context = src.task_context.fork(new_ksp, &self.trap_context);
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
            let mut space = self.user_space.lock(place!());
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
        let alive = Box::new(AliveTaskControlBlock {
            tid,
            stack_id,
            thread_group,
            kernel_stack,
            user_space: Arc::new(SpinLock::new(user_space)),
            _no_pin_marker: PhantomPinned,
            inner: SpinLock::new(TaskControlBlockInner::new()),
        });
        let (argc, argv) = (0, 0);
        let inner = unsafe { alive.inner.assert_unique_get() as *mut TaskControlBlockInner };
        let tcb = Arc::new(Self {
            pid,
            task_status: AtomicTaskStatus::from(TaskStatus::RUNNING),
            parent: SpinLock::new(None),
            exit_code: AtomicI32::new(0),
            alive: Some(alive),
        });
        unsafe { &mut *inner }.exec_init(kernel_sp, entry_point, user_sp, tcb.as_ref(), argc, argv);
        Ok(tcb)
    }
    pub fn run_in_this_stack(&self) -> bool {
        let stack = &self.alive.as_ref().unwrap().as_ref().kernel_stack;
        let begin = stack.addr_begin().into_usize();
        let end = stack.bottom().into_usize();
        let sp = riscv::current_sp();
        begin < sp && sp <= end
    }
    pub fn try_alive(&self) -> Option<&AliveTaskControlBlock> {
        // debug_check!(self.run_in_this_stack());
        self.alive.as_ref().map(|a| a.as_ref())
    }
    // forbid another hart run this.
    pub fn alive(&self) -> &AliveTaskControlBlock {
        debug_check!(self.run_in_this_stack());
        unsafe { self.alive_uncheck() }
    }
    pub unsafe fn alive_uncheck(&self) -> &AliveTaskControlBlock {
        debug_run! {{
            let flag = self.task_status.load();
            assert!(flag != TaskStatus::ZOMBIE && flag != TaskStatus::DEAD);
        }};
        // debug_check_eq!(self.trap_context_ptr());
        self.alive.as_ref().unwrap()
    }
    pub fn lock(&self) -> MutexGuard<TaskControlBlockInner, Spin> {
        self.alive().inner.lock(place!())
    }
    // pub fn trap_context_ptr(&self) -> *mut TrapContext {
    //     unsafe { &mut (*self.alive().inner.get_ptr()).trap_context }
    // }
    pub fn task_context_ptr(&self) -> *mut TaskContext {
        unsafe { &mut (*self.alive().inner.get_ptr()).task_context }
    }
    pub fn task_context_ptr_scheduler(&self) -> *mut TaskContext {
        unsafe { &mut (*self.alive_uncheck().inner.get_ptr()).task_context }
    }
    // call by scheduler so don't use alive.
    pub fn using_space_scheduler(&self) {
        unsafe { self.alive_uncheck().user_space.lock(place!()).using() };
    }
    // call by scheduler so don't use alive.
    pub fn using_space(&self) {
        self.alive().user_space.lock(place!()).using();
    }
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }
    pub fn is_zombie(&self) -> bool {
        self.task_status.load() == TaskStatus::ZOMBIE
    }
    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::Acquire)
    }
    pub fn set_parent(&self, parent: &Arc<Self>) {
        let weak = Some(Arc::downgrade(parent));
        *self.parent.lock(place!()) = weak;
    }
    pub fn kernel_bottom(&self) -> KernelAddr4K {
        self.alive().kernel_stack.bottom()
    }
    pub fn try_kernel_bottom(&self) -> Option<KernelAddr4K> {
        self.try_alive().map(|x| x.kernel_stack.bottom())
    }

    pub fn fork(
        &self,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(Arc<Self>, *mut TrapContext), TCBCreateError> {
        memory_trace!("TaskControlBlock::fork");
        allocator::heap_space_enough()?;
        let alive = self.alive();
        let user_space = alive.user_space.lock(place!()).fork(allocator)?;
        let kernel_stack = stack::alloc_kernel_stack()?;
        let pid_handle = pid::pid_alloc();
        let pid = pid_handle.pid();

        let thread_group = Arc::new(LockedThreadGroup::new(pid));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.

        let stack_id = alive.stack_id;
        let kernel_sp = kernel_stack.bottom();
        let new = Arc::new(Self {
            pid: pid_handle,
            task_status: AtomicTaskStatus::from(TaskStatus::RUNNING),
            parent: SpinLock::new(Some(Arc::downgrade(&get_current_task()))), // move parent
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
        allocator::heap_space_enough()?;
        let new_alive = unsafe { new.alive_uncheck() };
        let inner = unsafe { new_alive.inner.assert_unique_get() };
        let mut self_inner = alive.inner.lock(place!());
        inner.fork_init(&self_inner, kernel_sp, new.as_ref());
        let new_trap_cx_ptr = &mut inner.trap_context as *mut _;
        self_inner.children.push(new.clone());
        memory_trace!("TaskControlBlock::fork");
        // let this = get_current_task();
        // println!("fork this rc: {}", Arc::strong_count(&this));
        Ok((new, new_trap_cx_ptr))
    }
    pub fn copy_sx_from(&self, trap_context: &mut TrapContext) -> &Self {
        let inner = unsafe { self.alive().inner.assert_unique_get() };
        inner.trap_context.copy_sx_from(trap_context);
        self
    }
    pub fn fork_set_new_ret(&self, a0: usize) -> &Self {
        let inner = unsafe { self.alive_uncheck().inner.assert_unique_get() };
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
        // println!("TCB exec 0 hart = {}", cpu::hart_id());
        let alive = self.alive();
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator)?;
        // assume only 1 process
        {
            let mut cur_space = alive.user_space.lock(place!());
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
            let mut inner = alive.inner.lock(place!());
            inner.exec_init(kernel_sp, entry_point, user_sp, self, argc, argv);
            &mut inner.task_context as *mut TaskContext
        };
        // ERROR when hart = 4
        self.using_space();
        memory_trace!("TaskControlBlock::exec 2");
        sfence::fence_i();
        // println!("TCB exec end! goto task");
        unsafe { switch::goto_task(ncx) }
    }
    pub fn exit(&self, exit_code: i32) {
        memory_trace!("TaskControlBlock::exit entry");
        self.exit_code.store(exit_code, Ordering::Relaxed);
        {
            let mut children = core::mem::take(&mut self.alive().inner.lock(place!()).children);
            let initproc = get_initproc();
            let inner = &mut initproc.alive().inner.lock(place!()).children;
            while let Some(child) = children.pop() {
                child.set_parent(&initproc);
                inner.push(child);
            }
        }
        // unsafe!!!!! other hard need lock read status.
        let x = &self.alive as *const _ as *mut Option<Box<AliveTaskControlBlock>>;
        unsafe {
            assert!((*x).is_some());
            *x = None
        };
        self.task_status.store(TaskStatus::ZOMBIE);
        memory_trace!("TaskControlBlock::exit return");
    }
}
