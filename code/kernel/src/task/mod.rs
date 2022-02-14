mod context;
mod pid;
mod switch;
mod thread;
mod tid;

use core::{cell::UnsafeCell, marker::PhantomPinned, ptr};

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
    scheduler,
    sync::mutex::SpinLock,
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

/// TaskControlBlock
pub struct TaskControlBlock {
    pid: PidHandle,
    alive: UnsafeCell<Option<Box<AliveTaskControlBlock>>>, // if is none will become zombies
    exit_code: UnsafeCell<i32>,
}

unsafe impl Send for TaskControlBlock {}
unsafe impl Sync for TaskControlBlock {}

impl Drop for TaskControlBlock {
    fn drop(&mut self) {
        if PRINT_DROP_TCB {
            println!("TCB drop! pid: {:?}", self.pid());
        }
    }
}

struct ATCBbeforeExec(Box<AliveTaskControlBlock>);
impl ATCBbeforeExec {
    pub fn set_tcb_ptr(mut self, tcb_ptr: *const TaskControlBlock) -> Box<AliveTaskControlBlock> {
        self.0.trap_context.set_tcb_ptr(tcb_ptr);
        self.0
    }
}
struct ATCBbeforeFork(Box<AliveTaskControlBlock>);
impl ATCBbeforeFork {
    pub fn init(mut self, tcb_ptr: *const TaskControlBlock) -> Box<AliveTaskControlBlock> {
        self.0.trap_context.set_tcb_ptr(tcb_ptr);
        self.0
    }
}

/// only scheduler can touch this.
struct AliveTaskControlBlock {
    task_status: TaskStatus,
    tid: Tid,
    thread_group: Arc<LockedThreadGroup>,
    kernel_stack: KernelStackTracker,
    stack_id: StackID,
    user_space: Arc<SpinLock<UserSpace>>, // share with thread group
    parent: Option<Weak<TaskControlBlock>>,
    children: Vec<Arc<TaskControlBlock>>,
    trap_context: TrapContext, // switch to user
    task_context: TaskContext, // switch to scheduler
    _pin_marker: PhantomPinned,
}

impl AliveTaskControlBlock {
    /// need init parent ptb
    fn exec_new(
        pid: Pid,
        tcb_ptr: *const TaskControlBlock,
        elf_data: &[u8],
        allocator: &mut impl FrameAllocator,
    ) -> Result<Box<Self>, TCBCreateError> {
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator).map_err(|e| TCBCreateError::UserSpace(e))?;
        let thread_group = Arc::new(LockedThreadGroup::new(pid));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.
        let kernel_stack = stack::alloc_kernel_stack()?;
        let kernel_sp = kernel_stack.bottom();
        let mut alive = Box::new(AliveTaskControlBlock {
            tid,
            stack_id,
            thread_group,
            kernel_stack,
            user_space: Arc::new(SpinLock::new(user_space)),
            task_status: TaskStatus::RUNNING,
            parent: None,
            children: Vec::new(),
            trap_context: unsafe { TrapContext::any() },
            task_context: unsafe { TaskContext::any() },
            _pin_marker: PhantomPinned,
        });
        let (argc, argv) = (0, 0);
        alive.exec_init(kernel_sp, entry_point, user_sp, tcb_ptr, argc, argv);
        Ok(alive)
    }
    pub fn exec_init(
        &mut self,
        kernel_sp: KernelAddr4K,
        entry_point: UserAddr,
        user_sp: UserAddr4K,
        tcb_ptr: *const TaskControlBlock,
        argc: usize,
        argv: usize,
    ) {
        self.task_context.exec_init(kernel_sp, &self.trap_context);
        self.trap_context
            .exec_init(entry_point, user_sp, kernel_sp, tcb_ptr, argc, argv);
    }
    /// set parent = get_current_task
    pub fn fork(
        &self,
        pid: Pid,
        tcb: *const TaskControlBlock,
        allocator: &mut impl FrameAllocator,
    ) -> Result<Box<Self>, TCBCreateError> {
        let user_space = self.user_space.lock(place!()).fork(allocator)?;
        let kernel_stack = stack::alloc_kernel_stack()?;
        let new_kernel_sp = kernel_stack.bottom();

        let thread_group = Arc::new(LockedThreadGroup::new(pid));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.

        let mut alive = Box::new(AliveTaskControlBlock {
            tid,
            stack_id: self.stack_id,
            thread_group,
            kernel_stack,
            user_space: Arc::new(SpinLock::new(user_space)),
            task_status: TaskStatus::RUNNING,
            parent: Some(Arc::downgrade(&scheduler::get_current_task())),
            children: Vec::new(),
            trap_context: unsafe { TrapContext::any() },
            task_context: unsafe { TaskContext::any() },
            _pin_marker: PhantomPinned,
        });
        alive.trap_context = self.trap_context.fork_no_sx(new_kernel_sp, tcb);
        alive.task_context = self.task_context.fork(new_kernel_sp, &alive.trap_context);
        Ok(alive)
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
        let pid_handle = pid::pid_alloc();
        let pid = pid_handle.pid();
        let tcb = Arc::new(Self {
            pid: pid_handle,
            alive: UnsafeCell::new(None),
            exit_code: UnsafeCell::new(0),
        });
        let alive = AliveTaskControlBlock::exec_new(pid, tcb.as_ref(), elf_data, allocator)?;
        unsafe { *tcb.alive.get() = Some(alive) };
        // exec_init(kernel_sp, entry_point, user_sp, tcb.as_ref(), argc, argv);
        Ok(tcb)
    }
    pub fn run_in_this_stack(&self) -> bool {
        let stack = unsafe { &(*self.alive.get()).as_ref().unwrap().as_ref().kernel_stack };
        let begin = stack.addr_begin().into_usize();
        let end = stack.bottom().into_usize();
        let sp = riscv::current_sp();
        begin < sp && sp <= end
    }
    unsafe fn try_alive_uncheck(&self) -> Option<&AliveTaskControlBlock> {
        // debug_check!(self.run_in_this_stack());
        // self.alive.as_ref().map(|a| a.as_ref())
        (*self.alive.get()).as_ref().map(|a| a.as_ref())
    }
    // forbid another hart run this.
    fn alive(&self) -> &AliveTaskControlBlock {
        debug_check!(self.run_in_this_stack());
        unsafe { self.alive_uncheck() }
    }
    // forbid another hart run this.
    fn alive_mut(&self) -> &mut AliveTaskControlBlock {
        debug_check!(self.run_in_this_stack());
        unsafe { self.alive_mut_uncheck() }
    }
    unsafe fn alive_uncheck(&self) -> &AliveTaskControlBlock {
        (*self.alive.get()).as_ref().unwrap()
    }
    unsafe fn alive_mut_uncheck(&self) -> &mut AliveTaskControlBlock {
        (*self.alive.get()).as_mut().unwrap()
    }
    pub fn task_context_ptr(&self) -> *mut TaskContext {
        &mut (*self.alive_mut()).task_context
    }
    pub fn task_context_ptr_scheduler(&self) -> *mut TaskContext {
        unsafe { &mut (*self.alive_mut_uncheck()).task_context }
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
        unsafe { (*self.alive.get()).is_none() }
    }
    pub fn exit_code(&self) -> i32 {
        debug_check!(self.is_zombie());
        unsafe { *self.exit_code.get() }
    }
    pub fn set_parent(&self, parent: &Arc<Self>) {
        debug_check!(self.run_in_this_stack());
        let weak = Some(Arc::downgrade(parent));
        self.alive_mut().parent = weak;
    }
    pub fn kernel_bottom(&self) -> KernelAddr4K {
        unsafe { self.alive_uncheck() }.kernel_stack.bottom()
    }
    pub fn try_kernel_bottom(&self) -> Option<KernelAddr4K> {
        unsafe { self.try_alive_uncheck() }.map(|x| x.kernel_stack.bottom())
    }

    pub fn fork(
        &self,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(Arc<Self>, *mut TrapContext), TCBCreateError> {
        memory_trace!("TaskControlBlock::fork");
        assert!(self.run_in_this_stack());
        allocator::heap_space_enough()?;
        let pid_handle = pid::pid_alloc();
        let pid = pid_handle.pid();

        let new_tcb = Arc::new(TaskControlBlock {
            pid: pid_handle,
            alive: UnsafeCell::new(None),
            exit_code: UnsafeCell::new(0),
        });
        let self_alive = self.alive_mut();
        let mut new_alive = self_alive.fork(pid, new_tcb.as_ref(), allocator)?;
        let new_trap_cx_ptr = &mut new_alive.trap_context as *mut _;
        unsafe { *new_tcb.alive.get() = Some(new_alive) };
        self_alive.children.push(new_tcb.clone());

        allocator::heap_space_enough()?;

        memory_trace!("TaskControlBlock::fork");
        Ok((new_tcb, new_trap_cx_ptr))
    }

    pub fn copy_sx_from(&self, trap_context: &mut TrapContext) -> &Self {
        self.alive_mut().trap_context.copy_sx_from(trap_context);
        self
    }
    pub fn fork_set_new_ret(&self, a0: usize) -> &Self {
        unsafe { self.alive_mut_uncheck() }.trap_context.set_a0(a0);
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
        assert!(self.run_in_this_stack());
        // println!("TCB exec 0 hart = {}", cpu::hart_id());
        let alive = self.alive_mut();
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
        alive.exec_init(kernel_sp, entry_point, user_sp, self, argc, argv);

        let ncx = &mut alive.task_context as *mut TaskContext;
        // ERROR when hart = 4
        self.using_space();
        memory_trace!("TaskControlBlock::exec 2");
        sfence::fence_i();
        // println!("TCB exec end! goto task");
        unsafe { switch::goto_task(ncx) }
    }
    pub fn exit(&self, exit_code: i32) {
        memory_trace!("TaskControlBlock::exit entry");
        assert!(self.run_in_this_stack());
        unsafe { *self.exit_code.get() = exit_code };
        {
            todo!()
            // let mut children = core::mem::take(&mut self.alive().inner.lock(place!()).children);
            // let initproc = get_initproc();
            // let inner = &mut initproc.alive().inner.lock(place!()).children;
            // while let Some(child) = children.pop() {
            //     child.set_parent(&initproc);
            //     inner.push(child);
            // }
        }
        assert!(self.alive.get_mut().is_some());
        *self.alive.get_mut() = None;

        memory_trace!("TaskControlBlock::exit return");
    }
}
