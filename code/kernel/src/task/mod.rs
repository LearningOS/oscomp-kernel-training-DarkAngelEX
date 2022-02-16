pub mod children;
mod context;
mod msg;
mod pid;
mod switch;
mod thread;
mod tid;

use core::{cell::UnsafeCell, marker::PhantomPinned};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
    vec::Vec,
};
pub use switch::{goto_task, switch};

use crate::{
    config::PAGE_SIZE,
    xdebug::{stack_trace::StackTrace, NeverFail, PRINT_DROP_TCB},
    memory::{
        self,
        address::{KernelAddr4K, UserAddr, UserAddr4K},
        allocator::{
            self,
            frame::{self, FrameAllocator},
        },
        stack::{self, KernelStackTracker},
        StackID, USpaceCreateError, UserSpace,
    },
    message::{Message, MessageProcess, MessageReceive},
    riscv::{self, cpu, sfence},
    scheduler,
    sync::mutex::SpinLock,
    tools::error::{FrameOutOfMemory, HeapOutOfMemory},
    trap::context::TrapContext,
};

use self::{children::ChildrenSet, thread::LockedThreadGroup};
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
    msg_receive: MessageReceive,
    exit_code: UnsafeCell<i32>,
    // debug
    pub stack_trace: StackTrace,
}

unsafe impl Send for TaskControlBlock {}
unsafe impl Sync for TaskControlBlock {}

impl Drop for TaskControlBlock {
    fn drop(&mut self) {
        if PRINT_DROP_TCB {
            println!("TCB drop! pid: {:?}", self.pid());
        }
        assert!(self.msg_receive.is_close_or_empty());
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
pub struct AliveTaskControlBlock {
    task_status: TaskStatus,
    tid: Tid,
    thread_group: Arc<LockedThreadGroup>,
    kernel_stack: Option<KernelStackTracker>,
    stack_id: StackID,
    user_space: Arc<SpinLock<UserSpace>>, // share with thread group
    parent: Option<Weak<TaskControlBlock>>,
    children: ChildrenSet,
    msg_process: MessageProcess,
    trap_context: TrapContext, // switch to user
    task_context: TaskContext, // switch to scheduler
    _pin_marker: PhantomPinned,
}

impl Drop for AliveTaskControlBlock {
    fn drop(&mut self) {
        unsafe {
            stack_trace!();
            memory_trace!("AliveTaskControlBlock::drop begin");
            debug_check!(self.msg_process.is_empty());
            let allocator = &mut frame::defualt_allocator();
            let mut space = self.user_space.lock(place!());
            space.stack_dealloc(self.stack_id, allocator);
            self.thread_group.dealloc(self.tid);
            memory_trace!("AliveTaskControlBlock::drop end");
        }
    }
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
            kernel_stack: Some(kernel_stack),
            user_space: Arc::new(SpinLock::new(user_space)),
            task_status: TaskStatus::RUNNING,
            parent: None,
            children: ChildrenSet::new(),
            msg_process: MessageProcess::new(),
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
        stack_trace!();
        let user_space = self.user_space.lock(place!()).fork(allocator)?;
        let kernel_stack = stack::alloc_kernel_stack()?;
        let new_kernel_sp = kernel_stack.bottom();

        let thread_group = Arc::new(LockedThreadGroup::new(pid));
        let tid = thread_group.alloc(); // it doesn't matter if leak tid there.

        let mut alive = Box::new(AliveTaskControlBlock {
            tid,
            stack_id: self.stack_id,
            thread_group,
            kernel_stack: Some(kernel_stack),
            user_space: Arc::new(SpinLock::new(user_space)),
            task_status: TaskStatus::RUNNING,
            parent: Some(Arc::downgrade(&scheduler::get_current_task())),
            children: ChildrenSet::new(),
            msg_process: MessageProcess::new(),
            trap_context: unsafe { TrapContext::any() },
            task_context: unsafe { TaskContext::any() },
            _pin_marker: PhantomPinned,
        });
        alive.trap_context = self.trap_context.fork_no_sx(new_kernel_sp, tcb);
        alive.task_context = self.task_context.fork(new_kernel_sp, &alive.trap_context);
        Ok(alive)
    }
    unsafe fn take_kernel_stack(&mut self) -> KernelStackTracker {
        self.kernel_stack.take().unwrap()
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
            msg_receive: MessageReceive::new(),
            exit_code: UnsafeCell::new(0),
            stack_trace: StackTrace::new(),
        });
        let alive = AliveTaskControlBlock::exec_new(pid, tcb.as_ref(), elf_data, allocator)?;
        unsafe { *tcb.alive.get() = Some(alive) };
        // exec_init(kernel_sp, entry_point, user_sp, tcb.as_ref(), argc, argv);
        Ok(tcb)
    }
    pub fn run_in_this_stack(&self) -> bool {
        let stack = unsafe {
            &(*self.alive.get())
                .as_ref()
                .unwrap()
                .as_ref()
                .kernel_stack
                .as_ref()
                .unwrap()
        };
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
    pub fn alive(&self) -> &AliveTaskControlBlock {
        debug_check!(self.run_in_this_stack());
        unsafe { self.alive_uncheck() }
    }
    // forbid another hart run this.
    pub fn alive_mut(&self) -> &mut AliveTaskControlBlock {
        debug_check!(self.run_in_this_stack());
        unsafe { self.alive_mut_uncheck() }
    }
    unsafe fn alive_uncheck(&self) -> &AliveTaskControlBlock {
        debug_check!((*self.alive.get()).as_ref().is_some());
        (*self.alive.get()).as_ref().unwrap_unchecked()
    }
    unsafe fn alive_mut_uncheck(&self) -> &mut AliveTaskControlBlock {
        debug_check!((*self.alive.get()).as_ref().is_some());
        (*self.alive.get()).as_mut().unwrap_unchecked()
    }
    /// unsafe!!! must run before send change parent.
    unsafe fn become_zombie(&self) -> KernelStackTracker {
        stack_trace!();
        debug_check!(self.run_in_this_stack());
        let kernel_stack = self.alive_mut().take_kernel_stack();
        stack_trace!();
        *self.alive.get() = None;
        stack_trace!();
        kernel_stack
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
    // must run after receive message!!!
    pub fn assert_zombie(&self) {
        debug_check!(self.msg_receive.is_close());
        assert!(unsafe { (*self.alive.get()).is_none() });
    }
    // must run after receive message!!!
    pub fn exit_code(&self) -> i32 {
        debug_run!({ self.assert_zombie() });
        unsafe { *self.exit_code.get() }
    }
    pub fn set_parent(&self, parent: &Arc<Self>) {
        debug_check!(self.run_in_this_stack());
        let weak = Some(Arc::downgrade(parent));
        self.alive_mut().parent = weak;
    }
    pub fn kernel_bottom(&self) -> KernelAddr4K {
        unsafe { self.alive_uncheck() }
            .kernel_stack
            .as_ref()
            .unwrap()
            .bottom()
    }
    pub fn try_kernel_bottom(&self) -> Option<KernelAddr4K> {
        let atcb = unsafe { self.try_alive_uncheck()? };
        let stack = atcb.kernel_stack.as_ref()?;
        Some(stack.bottom())
    }

    pub fn fork(
        &self,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(Arc<Self>, *mut TrapContext), TCBCreateError> {
        stack_trace!();
        memory_trace!("TaskControlBlock::fork");
        assert!(self.run_in_this_stack());
        allocator::heap_space_enough()?;
        let pid_handle = pid::pid_alloc();
        let pid = pid_handle.pid();

        let new_tcb = Arc::new(TaskControlBlock {
            pid: pid_handle,
            alive: UnsafeCell::new(None),
            msg_receive: MessageReceive::new(),
            exit_code: UnsafeCell::new(0),
            stack_trace: StackTrace::new(),
        });
        let self_alive = self.alive_mut();
        let mut new_alive = self_alive.fork(pid, new_tcb.as_ref(), allocator)?;
        let new_trap_cx_ptr = &mut new_alive.trap_context as *mut _;
        unsafe { *new_tcb.alive.get() = Some(new_alive) };
        self_alive.children.push_child(new_tcb.clone());

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
        stack_trace!();
        memory_trace!("TaskControlBlock::exec 0");
        assert!(self.run_in_this_stack());
        // println!("TCB exec 0 hart = {}", cpu::hart_id());
        let alive = self.alive_mut();
        let (mut user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator)?;
        let never_fail = NeverFail::new();
        user_space.using();
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
        let kernel_sp = alive.kernel_stack.as_ref().unwrap().bottom();
        alive.exec_init(kernel_sp, entry_point, user_sp, self, argc, argv);

        let ncx = &mut alive.task_context as *mut TaskContext;
        // ERROR when hart = 4
        memory_trace!("TaskControlBlock::exec 2");
        never_fail.assume_success();
        sfence::fence_i();
        // println!("TCB exec end! goto task");
        unsafe { switch::goto_task(ncx) }
    }
    pub fn exit(&self, exit_code: i32) -> KernelStackTracker {
        stack_trace!();
        memory_trace!("TaskControlBlock::exit entry");
        assert!(self.run_in_this_stack());
        unsafe { *self.exit_code.get() = exit_code };
        // move all children to initproc and parent point
        let children = core::mem::take(&mut self.alive_mut().children);
        let mut change_parent = Vec::new();
        for (_pid, ptr) in children.alive_iter() {
            change_parent.push(ptr.clone());
        }
        let initproc = scheduler::get_initproc();

        initproc.receive_message_force(Message::MoveChildren(Box::new(children)));

        self.take_close_message(); // close message receive
        self.handle_message();
        let children = core::mem::take(&mut self.alive_mut().children);
        for (_pid, ptr) in children.alive_iter() {
            change_parent.push(ptr.clone());
        }
        initproc.receive_message_force(Message::MoveChildren(Box::new(children)));

        while let Some(child) = change_parent.pop() {
            // ignore error
            let _ = child.receive_message(Message::ChangeParent(Arc::downgrade(&initproc)));
        }

        let parent = self.alive().parent.as_ref().unwrap().upgrade().unwrap();
        // become_zombie must before send ChildBecomeZombie
        memory::set_satp_by_global();
        let kernel_stack = unsafe { self.become_zombie() };
        match parent.receive_message(Message::ChildBecomeZombie(self.pid())) {
            Ok(_) => (),
            Err(_) => initproc.receive_message_force(Message::ChildBecomeZombie(self.pid())),
        }

        memory_trace!("TaskControlBlock::exit return");
        kernel_stack
    }
}

impl TaskControlBlock {
    pub fn take_message(&self) {
        self.alive_mut().msg_process.take_from(&self.msg_receive);
    }
    pub fn take_close_message(&self) {
        self.alive_mut()
            .msg_process
            .take_and_close_from(&self.msg_receive);
    }
    pub fn handle_message(&self) {
        stack_trace!();
        let ref mut alive = self.alive_mut();
        let ref mut msgs = alive.msg_process;
        while let Some(msg) = msgs.pop() {
            match msg {
                Message::ChildBecomeZombie(pid) => {
                    let ref mut children = alive.children;
                    children.become_zombie(pid);
                }
                Message::MoveChildren(children) => {
                    alive.children.append(*children);
                }
                Message::ChangeParent(new_parent) => {
                    alive.parent = Some(new_parent);
                }
            }
        }
    }
    pub fn receive_message(&self, msg: Message) -> Result<(), Message> {
        self.msg_receive.receive(msg)
    }
    pub fn receive_message_force(&self, msg: Message) {
        stack_trace!();
        match self.msg_receive.receive(msg) {
            Ok(_) => (),
            Err(_) => panic!("receive_message_force"),
        }
    }
}

// for waitpid
impl TaskControlBlock {
    pub fn have_child_of(&self, pid: Pid) -> bool {
        self.alive_mut().children.have_child_of(pid)
    }
    pub fn no_children(&self) -> bool {
        self.alive_mut().children.no_children()
    }
    pub fn try_remove_zombie(&self, pid: Pid) -> Option<Arc<TaskControlBlock>> {
        self.alive_mut().children.try_remove_zombie(pid)
    }
    pub fn try_remove_zombie_any(&self) -> Option<Arc<TaskControlBlock>> {
        self.alive_mut().children.try_remove_zombie_any()
    }
}
