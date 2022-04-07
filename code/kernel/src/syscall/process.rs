use core::{
    convert::TryFrom,
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
        address::{PageCount, UserAddr},
        user_ptr::{UserInOutPtr, UserReadPtr, UserWritePtr},
        UserSpace,
    },
    process::{proc_table, thread, userloop, CloneFlag, Pid},
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
    pub fn sys_clone(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            print!("sys_clone {:?} ", self.process.pid());
        }
        let (flag, newsp, parent_tidptr, child_tidptr, _tls_val): (
            u32,
            usize,
            UserInOutPtr<u32>,
            UserInOutPtr<u32>,
            u32,
        ) = self.cx.into();

        // println!("{}clone: {:#x}{}", to_yellow!(), flag, reset_color!());
        let flag = CloneFlag::from_bits(flag).ok_or(SysError::EINVAL)?;
        let new = match self
            .thread
            .clone_impl(flag, newsp, parent_tidptr, child_tidptr)
        {
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
        let (path, args, envp): (
            UserReadPtr<u8>,
            UserReadPtr<UserReadPtr<u8>>,
            UserReadPtr<UserReadPtr<u8>>,
        ) = self.cx.para3();
        let user_check = UserCheck::new();
        let path = String::from_utf8(
            user_check
                .translated_user_array_zero_end(path)
                .await?
                .to_vec(),
        )?;
        let args: Vec<String> = user_check
            .translated_user_2d_array_zero_end(args)
            .await?
            .into_iter()
            .map(|a| unsafe { String::from_utf8_unchecked(a.to_vec()) })
            .collect();
        // println!("args ptr: {:#x}", args as usize);
        // let args = Vec::new();
        let envp = user_check
            .translated_user_2d_array_zero_end(envp)
            .await?
            .into_iter()
            .map(|a| unsafe { String::from_utf8_unchecked(a.to_vec()) })
            .collect::<Vec<String>>();

        let args_size = UserSpace::push_args_size(&args, &envp);
        let stack_reverse = args_size + PageCount(USER_STACK_RESERVE / PAGE_SIZE);
        let inode = fs::open_file(path.as_str(), fs::OpenFlags::RDONLY)?;
        let elf_data = inode.read_all().await;
        let (user_space, stack_id, user_sp, entry_point, auxv) =
            UserSpace::from_elf(elf_data.as_slice(), stack_reverse)
                .map_err(|_e| SysError::ENOEXEC)?;

        // TODO: kill other thread and await
        let mut alive = self.alive_lock()?;
        if alive.threads.len() > 1 {
            todo!();
        }
        let check = NeverFail::new();
        unsafe { user_space.using() };
        let (user_sp, argc, argv, envp) =
            user_space.push_args(user_sp.into(), &args, &alive.envp, &auxv, args_size);
        drop(auxv);
        drop(args);
        // reset stack_id
        alive.fd_table.exec_run();
        alive.exec_path = path;
        alive.user_space = user_space;
        alive.cwd = String::new();
        drop(alive);
        self.thread.inner().stack_id = stack_id;
        let cx = self.thread.get_context();
        let sstatus = cx.user_sstatus;
        let fcsr = cx.user_fx.fcsr;
        self.thread
            .get_context()
            .exec_init(user_sp, entry_point, sstatus, fcsr, argc, argv, envp);
        local::all_hart_fence_i();
        check.assume_success();
        Ok(argc)
    }
    pub async fn sys_waitpid(&mut self) -> SysResult {
        stack_trace!();
        let (pid, exit_code_ptr): (isize, UserWritePtr<i32>) = self.cx.para2();
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
    pub fn sys_set_tid_address(&mut self) -> SysResult {
        stack_trace!();
        let set_child_tid: UserInOutPtr<u32> = self.cx.para1();
        self.thread.inner().set_child_tid = set_child_tid;
        Ok(self.thread.tid.to_usize())
    }
    pub fn sys_getpid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getpid");
        }
        Ok(self.process.pid().into_usize())
    }
    pub fn sys_getuid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getuid");
        }
        Ok(0)
    }
    pub fn sys_exit(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_exit {:?}", self.process.pid());
        }
        debug_assert!(
            self.process.pid() != Pid::from_usize(0),
            "{}",
            to_red!("initproc exit")
        );
        self.do_exit = true;
        let exit_code: i32 = self.cx.para1();
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
    pub fn sys_exit_group(&mut self) -> SysResult {
        self.sys_exit()
    }
    pub async fn sys_yield(&mut self) -> SysResult {
        stack_trace!();
        thread::yield_now().await;
        Ok(0)
    }
    pub async fn sys_sleep(&mut self) -> SysResult {
        let millisecond: usize = self.cx.para1();
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
        let (pid, _signal): (isize, u32) = self.cx.para2();
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
                let _proc = proc_table::find_proc(pid).ok_or(SysError::ESRCH)?;
                todo!();
            }
            Target::AllInGroup => todo!(),
            Target::All => todo!(),
            Target::Group(_) => todo!(),
        }
    }
    pub fn sys_brk(&mut self) -> SysResult {
        stack_trace!();
        let brk: usize = self.cx.para1();
        println!("sys_brk: {:#x}", brk);
        let brk = if brk == 0 {
            self.alive_then(|a| a.user_space.get_brk())?
        } else {
            let brk = UserAddr::try_from(brk as *const ())?.ceil();
            self.alive_then(|a| a.user_space.reset_brk(brk))??;
            brk
        };
        println!("    -> {:#x}", brk.into_usize());
        Ok(brk.into_usize())
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
