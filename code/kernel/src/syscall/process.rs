use core::{
    future::Future,
    sync::atomic::Ordering,
    task::{Context, Poll},
};

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    config::{PAGE_SIZE, USER_STACK_RESERVE},
    fs, local,
    memory::{self, address::PageCount, allocator::frame, user_ptr::UserOutPtr, UserSpace},
    process::{thread, userloop, Pid},
    sync::{
        even_bus::{self, Event, EventBus},
        mutex::SpinNoIrqLock as Mutex,
    },
    timer::{self, TimeTicks},
    tools::allocator::from_usize_allocator::FromUsize,
    xdebug::{NeverFail, PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysError, SysResult, Syscall};

const PRINT_SYSCALL_PROCESS: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub fn sys_fork(&mut self) -> SysResult {
        if PRINT_SYSCALL_PROCESS {
            print!("sys_fork {:?} ", self.process.pid());
        }
        let allocator = &mut frame::defualt_allocator();
        let new = match self.thread.fork(allocator) {
            Ok(new) => new,
            Err(_e) => {
                println!("frame out of memory");
                return Err(SysError::ENOMEM);
            }
        };
        let pid = new.process.pid();
        userloop::spawn(new);
        if PRINT_SYSCALL_PROCESS {
            println!("-> {:?}", pid);
        }
        Ok(pid.into_usize())
    }
    pub async fn sys_exec(&mut self) -> SysResult {
        if PRINT_SYSCALL_PROCESS {
            println!("sys_exec {:?}", self.process.pid());
        }
        let (path, args, _envp) = {
            let (path, args, _envp): (*const u8, *const *const u8, *const *const u8) =
                self.cx.parameter3();
            let guard = self.using_space()?;
            let path =
                String::from_utf8(guard.translated_user_array_zero_end(path)?.into_vec(&guard))?;
            let args: Vec<String> = guard
                .translated_user_2d_array_zero_end(args)?
                .into_iter()
                .map(|a| unsafe { String::from_utf8_unchecked(a.into_vec(&guard)) })
                .collect();
            // println!("args ptr: {:#x}", args as usize);
            // let args = Vec::new();
            // let envp = user::translated_user_2d_array_zero_end(envp)?;
            (path, args, ())
        };
        let args_size = args.iter().fold(0, |n, s| n + s.len() + 1)
            + (args.len() + 1) * core::mem::size_of::<usize>();
        let stack_reverse =
            PageCount::from_usize((args_size + PAGE_SIZE - 1 + USER_STACK_RESERVE) / PAGE_SIZE);
        println!("stack_reverse: {:?}", stack_reverse);
        let inode = fs::open_file(path.as_str(), fs::OpenFlags::RDONLY).ok_or(SysError::ENFILE)?;
        let elf_data = inode.read_all().await;
        let allocator = &mut frame::defualt_allocator();
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data.as_slice(), stack_reverse, allocator)
                .map_err(|_e| SysError::ENOEXEC)?;
        let (user_sp, argc, argv) = user_space.push_args(args, user_sp.into());
        let check = NeverFail::new();
        // reset stack_id
        // user_space.using();
        // sfence::fence_i();
        local::all_hart_fence_i();
        let mut alive = self.alive_lock()?;
        // TODO: kill other thread and await
        if alive.threads.len() > 1 {
            todo!();
        }
        alive.exec_path = path;
        alive.user_space = user_space;
        drop(alive);
        self.thread.inner().stack_id = stack_id;
        let sstatus = self.thread.get_context().user_sstatus;
        self.thread
            .get_context()
            .exec_init(user_sp, entry_point, sstatus, argc, argv);

        check.assume_success();
        Ok(argc)
    }
    pub async fn sys_waitpid(&mut self) -> SysResult {
        let (pid, exit_code_ptr): (isize, UserOutPtr<i32>) = self.cx.parameter2();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_waitpid {:?} <- {}", self.process.pid(), pid);
        }
        enum WaitFor {
            PGid(usize),
            AnyChild,
            AnyChildInGroup,
            Pid(Pid),
        }
        let target = match pid {
            -1 => WaitFor::AnyChild,
            0 => WaitFor::AnyChildInGroup,
            p if p > 0 => WaitFor::Pid(Pid::from_usize(p as usize)),
            p => WaitFor::PGid(p as usize),
        };
        loop {
            // this brace is for xlock which drop before .await but stupid rust can't see it.
            {
                let this_pid = self.process.pid();
                let mut alive = self.alive_lock()?;
                let process = match target {
                    WaitFor::AnyChild => alive.children.try_remove_zombie_any(),
                    WaitFor::Pid(pid) => alive.children.try_remove_zombie(pid),
                    WaitFor::PGid(_) => unimplemented!(),
                    WaitFor::AnyChildInGroup => unimplemented!(),
                };
                if let Some(process) = process {
                    if let Some(exit_code_ptr) = exit_code_ptr.nonnull_mut() {
                        let exit_code = process.exit_code.load(Ordering::Relaxed);
                        let guard = alive.user_space.using_guard();
                        let access =
                            guard.translated_user_writable_slice(exit_code_ptr as *mut u8, 4)?;
                        let exit_code_slice =
                            core::ptr::slice_from_raw_parts(&exit_code as *const _ as *const u8, 4);
                        access
                            .access_mut(&guard)
                            .copy_from_slice(unsafe { &*exit_code_slice });
                    }
                    if PRINT_SYSCALL_PROCESS {
                        println!("sys_waitpid success {:?} <- {:?}", this_pid, process.pid());
                    }
                    return Ok(process.pid().into_usize());
                } else if alive.children.is_empty() {
                    return Err(SysError::ECHILD);
                }
                drop(alive);
            }
            let event_bus = &self.process.event_bus;
            if let Err(_e) = even_bus::wait_for_event(event_bus.clone(), Event::CHILD_PROCESS_QUIT)
                .await
                .and_then(|_x| event_bus.lock(place!()).clear(Event::CHILD_PROCESS_QUIT))
            {
                self.do_exit = true;
                return Err(SysError::ESRCH);
            }
            // check then
        }
    }
    pub fn sys_getpid(&mut self) -> SysResult {
        if PRINT_SYSCALL_ALL {
            println!("sys_getpid");
        }
        Ok(self.process.pid().into_usize())
    }
    pub fn sys_exit(&mut self) -> SysResult {
        stack_trace!(self.stack_trace.ptr_usize());
        if PRINT_SYSCALL_PROCESS {
            println!("sys_exit");
        }
        self.do_exit = true;
        let exit_code: i32 = self.cx.parameter1();
        self.process.event_bus.lock(place!()).close();
        let mut lock = self.process.alive.lock(place!());
        let alive = match lock.as_mut() {
            Some(a) => a,
            None => return Err(SysError::ESRCH),
        };
        self.process.exit_code.store(exit_code, Ordering::Relaxed);
        // TODO: waiting other thread exit

        memory::set_satp_by_global();
        alive.clear_all(self.process.pid());
        *lock = None;
        Ok(0)
    }
    pub async fn sys_yield(&mut self) -> SysResult {
        thread::yield_now().await;
        Ok(0)
    }
    pub async fn sys_sleep(&mut self) -> SysResult {
        let millisecond: usize = self.cx.parameter1();
        let time_now = timer::get_time_ticks();
        let deadline = time_now + TimeTicks::from_millisecond(millisecond);
        let future = SleepFuture {
            deadline,
            event_bus: self.process.event_bus.clone(),
        };
        future.await
    }
}

pub struct SleepFuture {
    deadline: TimeTicks,
    event_bus: Arc<Mutex<EventBus>>,
}

impl Future for SleepFuture {
    type Output = SysResult;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if timer::get_time_ticks() >= self.deadline {
            cx.waker().wake_by_ref();
            return Poll::Ready(Ok(0));
        } else if self.event_bus.lock(place!()).event != Event::empty() {
            return Poll::Ready(Err(SysError::EINTR));
        }
        timer::sleep::timer_push_task(self.deadline, cx.waker().clone());
        match self
            .event_bus
            .lock(place!())
            .register(Event::all(), cx.waker().clone())
        {
            Err(_e) => Poll::Ready(Err(SysError::ESRCH)),
            Ok(()) => Poll::Pending,
        }
    }
}
