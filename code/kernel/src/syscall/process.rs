use core::{
    future::Future,
    sync::atomic::Ordering,
    task::{Context, Poll},
};

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    config::{PAGE_SIZE, USER_STACK_RESERVE},
    fs, local,
    memory::{
        self,
        address::PageCount,
        allocator::frame,
        user_ptr::{UserReadPtr, UserWritePtr},
        UserSpace,
    },
    process::{proc_table, thread, userloop, Pid},
    sync::{
        even_bus::{self, Event, EventBus},
        mutex::SpinNoIrqLock as Mutex,
    },
    timer::{self, TimeTicks},
    tools::allocator::from_usize_allocator::FromUsize,
    user::check::UserCheck,
    xdebug::{NeverFail, PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysError, SysResult, Syscall};

const PRINT_SYSCALL_PROCESS: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub fn sys_fork(&mut self) -> SysResult {
        stack_trace!();
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
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_exec {:?}", self.process.pid());
        }
        let (path, args, _envp) = {
            let (path, args, _envp): (
                UserReadPtr<u8>,
                UserReadPtr<UserReadPtr<u8>>,
                UserReadPtr<UserReadPtr<u8>>,
            ) = self.cx.parameter3();
            let user_check = UserCheck::new();
            let path = String::from_utf8(
                user_check
                    .translated_user_array_zero_end(path)
                    .await?
                    .into_vec(),
            )?;
            let args: Vec<String> = user_check
                .translated_user_2d_array_zero_end(args)
                .await?
                .into_iter()
                .map(|a| unsafe { String::from_utf8_unchecked(a.into_vec()) })
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
        let inode = fs::open_file(path.as_str(), fs::OpenFlags::RDONLY).ok_or(SysError::ENFILE)?;
        let elf_data = inode.read_all().await;
        let allocator = &mut frame::defualt_allocator();
        let (user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data.as_slice(), stack_reverse, allocator)
                .map_err(|_e| SysError::ENOEXEC)?;

        // TODO: kill other thread and await
        let mut alive = self.alive_lock()?;
        if alive.threads.len() > 1 {
            todo!();
        }
        let check = NeverFail::new();
        unsafe { user_space.using() };
        let (user_sp, argc, argv) = user_space.push_args(args, user_sp.into());
        // reset stack_id
        alive.exec_path = path;
        alive.user_space = user_space;
        drop(alive);
        self.thread.inner().stack_id = stack_id;
        let sstatus = self.thread.get_context().user_sstatus;
        self.thread
            .get_context()
            .exec_init(user_sp, entry_point, sstatus, argc, argv);
        local::all_hart_fence_i();
        check.assume_success();
        Ok(argc)
    }
    pub async fn sys_waitpid(&mut self) -> SysResult {
        stack_trace!();
        let (pid, exit_code_ptr): (isize, UserWritePtr<i32>) = self.cx.parameter2();
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

            let this_pid = self.process.pid();

            let process = {
                let mut alive = self.alive_lock()?;
                let p = match target {
                    WaitFor::AnyChild => alive.children.try_remove_zombie_any(),
                    WaitFor::Pid(pid) => alive.children.try_remove_zombie(pid),
                    WaitFor::PGid(_) => unimplemented!(),
                    WaitFor::AnyChildInGroup => unimplemented!(),
                };
                if p.is_none() && alive.children.is_empty() {
                    return Err(SysError::ECHILD);
                }
                p
            };
            if let Some(process) = process {
                if let Some(exit_code_ptr) = exit_code_ptr.transmute::<u8>().nonnull_mut() {
                    let exit_code = process.exit_code.load(Ordering::Relaxed);
                    // assert!(alive.user_space.in_using());
                    let access = UserCheck::new()
                        .translated_user_writable_slice(exit_code_ptr, 4)
                        .await?;
                    let exit_code_slice =
                        core::ptr::slice_from_raw_parts(&exit_code as *const _ as *const u8, 4);
                    access
                        .access_mut()
                        .copy_from_slice(unsafe { &*exit_code_slice });
                }
                if PRINT_SYSCALL_PROCESS {
                    println!("sys_waitpid success {:?} <- {:?}", this_pid, process.pid());
                }
                return Ok(process.pid().into_usize());
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
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getpid");
        }
        Ok(self.process.pid().into_usize())
    }
    pub fn sys_exit(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_exit {:?}", self.process.pid());
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
        stack_trace!();
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
    pub fn sys_kill(&mut self) -> SysResult {
        stack_trace!();
        let (pid, signal): (isize, u32) = self.cx.parameter2();
        enum Target {
            Pid(Pid),     // > 0
            AllInGroup,   // == 0
            All,          // == -1 all have authority except initproc
            Group(usize), // < -1
        }
        let target = match pid {
            0 => Target::AllInGroup,
            -1 => Target::All,
            p if p > 0 => Target::Pid(Pid::from_usize(p as usize)),
            g => Target::Group(-g as usize),
        };
        match target {
            Target::Pid(pid) => {
                let proc = proc_table::find_proc(pid).ok_or(SysError::ESRCH)?;
                todo!();
            }
            Target::AllInGroup => todo!(),
            Target::All => todo!(),
            Target::Group(_) => todo!(),
        }
    }
}

pub struct SleepFuture {
    deadline: TimeTicks,
    event_bus: Arc<Mutex<EventBus>>,
}

impl Future for SleepFuture {
    type Output = SysResult;

    fn poll(self: core::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        stack_trace!();
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
