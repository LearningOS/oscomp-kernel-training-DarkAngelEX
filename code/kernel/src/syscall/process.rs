use core::{convert::TryFrom, ops::Deref, sync::atomic::Ordering, time::Duration};

use alloc::{string::String, vec::Vec};
use ftl_util::{
    async_tools,
    fs::{Mode, OpenFlags},
    time::TimeSpec,
};

use crate::{
    config::{PAGE_SIZE, USER_DYN_BEGIN, USER_STACK_RESERVE},
    fs, local,
    memory::{
        address::{PageCount, UserAddr},
        allocator::frame,
        asid::USING_ASID,
        auxv::{AuxHeader, AT_BASE},
        user_ptr::{UserInOutPtr, UserReadPtr, UserWritePtr},
        UserSpace,
    },
    process::{search, thread, userloop, CloneFlag, Pid},
    sync::even_bus::{self, Event},
    timer,
    tools::allocator::from_usize_allocator::FromUsize,
    user::check::UserCheck,
    xdebug::{NeverFail, PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysError, SysRet, Syscall};

const PRINT_SYSCALL_PROCESS: bool = false || true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub async fn sys_clone(&mut self) -> SysRet {
        stack_trace!();
        let (flag, new_sp, ptid, tls, ctid): (
            usize,
            usize,
            UserInOutPtr<u32>,
            UserInOutPtr<u8>,
            UserInOutPtr<u32>,
        ) = self.cx.into();
        const PRINT_THIS: bool = false;
        if PRINT_SYSCALL_PROCESS || PRINT_THIS {
            println!(
                "sys_clone by {:?} sig: {} flag: {:?}\n\tsp: {:#x} ptid: {:#x} tls: {:#x} ctid: {:#x}",
                self.process.pid(),
                flag & 0xff,
                CloneFlag::from_bits(flag as u64).unwrap(),
                new_sp,
                ptid.as_usize(),
                ctid.as_usize(),
                tls.as_usize()
            );
        }
        let flag = CloneFlag::from_bits(flag as u64).ok_or(SysError::EINVAL)?;

        let set_child_tid = flag
            .contains(CloneFlag::CLONE_CHILD_SETTID)
            .then_some(ctid)
            .unwrap_or_else(UserInOutPtr::null);

        let clear_child_tid = flag
            .contains(CloneFlag::CLONE_CHILD_CLEARTID)
            .then_some(ctid)
            .unwrap_or_else(UserInOutPtr::null);

        let tls = flag.contains(CloneFlag::CLONE_SETTLS).then_some(tls);

        let exit_signal = flag
            .contains(CloneFlag::CLONE_DETACHED)
            .then_some(0)
            .unwrap_or((flag & CloneFlag::EXIT_SIGNAL).bits() as u32);

        let new = match flag.contains(CloneFlag::CLONE_THREAD) {
            true => self.thread.clone_thread(
                flag,
                new_sp,
                set_child_tid,
                clear_child_tid,
                tls,
                exit_signal,
            )?,
            false => self.thread.fork_impl(
                flag,
                new_sp,
                set_child_tid,
                clear_child_tid,
                tls,
                exit_signal,
            )?,
        };
        let tid = new.tid();
        if flag.contains(CloneFlag::CLONE_PARENT_SETTID) {
            match UserCheck::new(self.process).writable_value(ptid).await {
                Ok(ptid) => ptid.store(new.tid().0 as u32),
                Err(_e) => {
                    println!("sys_clone ignore error: CLONE_PARENT_SETTID fail");
                }
            }
        }
        userloop::spawn(new);
        local::try_wake_idle_hart();
        if PRINT_SYSCALL_PROCESS || PRINT_THIS {
            println!("\t-> {:?}", tid);
        }
        Ok(tid.0)
    }
    pub async fn sys_execve(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_execve {:?}", self.process.pid());
        }
        let (path, args, envp): (
            UserReadPtr<u8>,
            UserReadPtr<UserReadPtr<u8>>,
            UserReadPtr<UserReadPtr<u8>>,
        ) = self.cx.into();
        let user_check = UserCheck::new(self.process);
        let mut path = String::from_utf8(user_check.array_zero_end(path).await?.to_vec())?;
        stack_trace!("sys_execve path: {}", path);
        let mut args: Vec<String> = user_check
            .array_2d_zero_end(args)
            .await?
            .into_iter()
            .map(|a| unsafe { String::from_utf8_unchecked(a.to_vec()) })
            .collect();
        let envp = user_check
            .array_2d_zero_end(envp)
            .await?
            .into_iter()
            .map(|a| unsafe { String::from_utf8_unchecked(a.to_vec()) })
            .collect::<Vec<String>>();
        if PRINT_SYSCALL_PROCESS {
            println!("execve path {:?} args: {:?}", path, args);
            // println!("envp: {:?}", envp);
        }
        if path.ends_with(".sh") {
            args.insert(0, String::from("/busybox"));
            args.insert(1, String::from("sh"));
            path = String::from("/busybox");
        }
        let args_size = UserSpace::push_args_size(&args, &envp);
        let stack_reverse = args_size + PageCount(USER_STACK_RESERVE / PAGE_SIZE);
        let inode = fs::open_file(
            (Ok(self.alive_then(|a| a.cwd.clone())), path.as_str()),
            OpenFlags::RDONLY,
            Mode(0o500),
        )
        .await?;
        let dir = inode.parent()?.unwrap();
        let elf_data = inode.read_all().await?;
        let (mut user_space, user_sp, mut entry_point, mut auxv) =
            UserSpace::from_elf(elf_data.as_slice(), stack_reverse)
                .map_err(|_e| SysError::ENOEXEC)?;
        if PRINT_SYSCALL_PROCESS {
            println!("entry 0: {:#x}", entry_point.into_usize());
        }
        if let Some(dyn_entry_point) = user_space.load_linker(&elf_data).await.unwrap() {
            entry_point = dyn_entry_point;
            if PRINT_SYSCALL_PROCESS {
                println!("entry link: {:#x}", entry_point.into_usize());
            }
            auxv.push(AuxHeader {
                aux_type: AT_BASE,
                value: USER_DYN_BEGIN,
            });
        }
        // TODO: kill other thread and await
        let mut alive = self.alive_lock();
        if alive.threads.len() > 1 {
            todo!();
        }
        let check = NeverFail::new();
        if !USING_ASID {
            local::all_hart_sfence_vma_asid(alive.asid());
        }
        unsafe { user_space.using() };
        let (user_sp, argc, argv, envp) =
            user_space.push_args(user_sp, &args, &envp, &auxv, args_size);
        drop(auxv);
        drop(args);
        // reset stack_id
        alive.fd_table.exec_run();
        alive.exec_path = path;
        alive.user_space = user_space;
        alive.cwd = dir;
        drop(alive);
        self.process.signal_manager.reset();
        let cx = self.thread.get_context();
        let sstatus = cx.user_sstatus;
        let fcsr = cx.user_fx.fcsr;
        cx.exec_init(user_sp, entry_point, sstatus, fcsr, (argc, argv, envp));
        local::all_hart_fence_i();
        check.assume_success();
        // rtld_fini: 动态链接器析构函数
        // 如果这个值非零, glibc会在程序结束时把它当作函数指针并运行
        let rtld_fini = 0;
        Ok(rtld_fini)
    }
    pub async fn sys_wait4(&mut self) -> SysRet {
        stack_trace!();
        let (pid, exit_code_ptr, _option, _rusage): (
            isize,
            UserWritePtr<u32>,
            u32,
            UserWritePtr<u8>,
        ) = self.cx.into();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_wait4 {:?} <- {}", self.process.pid(), pid);
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
        let mut waker = None;
        loop {
            let this_pid = self.process.pid();
            let process = {
                // 这里不能用alive_then, 因为children可能被子进程修改
                let mut alive = self.alive_lock();
                let p = match target {
                    WaitFor::AnyChild => alive.children.try_remove_zombie_any(),
                    WaitFor::Pid(pid) => alive.children.try_remove_zombie(pid),
                    WaitFor::PGid(_) => unimplemented!(),
                    WaitFor::AnyChildInGroup => unimplemented!(),
                };
                if p.is_none() && alive.children.is_empty() {
                    if PRINT_SYSCALL_PROCESS {
                        println!("[FTL OS]wait4 fail: no child");
                    }
                    return Err(SysError::ECHILD);
                }
                p
            };
            if let Some(process) = process {
                // 找到了一个子进程
                let timer_sub = *process.timer.lock();
                self.process.timer.lock().append_child(&timer_sub);
                if let Some(exit_code_ptr) = exit_code_ptr.nonnull_mut() {
                    let exit_code = process.exit_code.load(Ordering::Relaxed);
                    let access = UserCheck::new(self.process)
                        .writable_value(exit_code_ptr)
                        .await
                        .map_err(|e| {
                            println!("[FTL OS]wait4 fail because {:?}", e);
                            e
                        })?;
                    let status: u8 = 0;
                    let wstatus = ((exit_code as u32 & 0xff) << 8) | (status as u32);
                    access.store(wstatus);
                }
                if PRINT_SYSCALL_PROCESS {
                    println!(
                        "sys_wait4 success {:?} <- {:?} (exit code {})",
                        this_pid,
                        process.pid(),
                        process.exit_code.load(Ordering::Relaxed)
                    );
                }
                return Ok(process.pid().0);
            }
            let event_bus = &self.process.event_bus;
            if waker.is_none() {
                waker = Some(async_tools::take_waker().await);
            }
            let _event = even_bus::wait_for_event(
                event_bus,
                Event::CHILD_PROCESS_QUIT,
                &waker.as_ref().unwrap(),
            )
            .await;
            event_bus.clear(Event::CHILD_PROCESS_QUIT).unwrap();
        }
    }
    pub fn sys_set_tid_address(&mut self) -> SysRet {
        stack_trace!();
        let clear_child_tid: UserInOutPtr<u32> = self.cx.para1();
        if PRINT_SYSCALL_ALL {
            println!("sys_set_tid_address {:#x}", clear_child_tid.as_usize());
        }
        self.thread.inner().clear_child_tid = clear_child_tid;
        Ok(self.thread.tid().0)
    }
    /// 设置pgid
    ///
    /// 如果pid为0则处理本进程, 如果pgid为0则设置为pid
    pub fn sys_setpgid(&mut self) -> SysRet {
        let (pid, pgid): (Pid, Pid) = self.cx.into();
        if PRINT_SYSCALL_ALL {
            println!("sys_setpgid pid: {:?} pgid: {:?}", pid, pgid);
        }
        let process = match pid {
            Pid(0) => None,
            pid => Some(search::find_proc(pid).ok_or(SysError::ESRCH)?),
        };
        let process = match &process {
            None => self.process,
            Some(p) => p.deref(),
        };
        let pgpid = match pgid {
            Pid(0) => process.pid(),
            pgid => pgid,
        };
        process.pgid.store(pgpid.0, Ordering::Relaxed);
        Ok(0)
    }
    /// 获取pgid
    ///
    /// 如果参数为0则获取自身进程pgid
    pub fn sys_getpgid(&mut self) -> SysRet {
        stack_trace!();
        let pid: Pid = self.cx.para1();
        if PRINT_SYSCALL_ALL {
            println!("sys_getpgid pid: {:?}", pid);
        }
        let pid = match pid {
            Pid(0) => self.process.pgid.load(Ordering::Relaxed),
            pid => search::find_proc(pid)
                .ok_or(SysError::ESRCH)?
                .pgid
                .load(Ordering::Relaxed),
        };
        Ok(pid)
    }
    pub fn sys_getpid(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getpid -> {:?}", self.process.pid());
        }
        Ok(self.process.pid().0)
    }
    pub fn sys_getppid(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getppid");
        }
        let pid = self
            .alive_then(|a| {
                a.parent
                    .as_ref()
                    .and_then(|p| p.upgrade())
                    .map(|p| p.pid().0)
            })
            .unwrap_or(0); // initproc
        Ok(pid)
    }
    pub fn sys_getuid(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getuid");
        }
        Ok(0)
    }
    pub fn sys_geteuid(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_geteuid");
        }
        Ok(0)
    }
    pub fn sys_getegid(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getegid");
        }
        Ok(0)
    }
    pub fn sys_exit(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_exit {:?} {:?}", self.process.pid(), self.thread.tid());
        }
        let exit_code: i32 = self.cx.para1();
        debug_assert!(self.process.pid() != Pid(0), "{}", to_red!("initproc exit"));
        self.process.exit_code.store(exit_code, Ordering::Release);
        self.do_exit = true;
        self.thread.inner().exited = true;
        Ok(0)
    }
    pub fn sys_exit_group(&mut self) -> SysRet {
        stack_trace!();
        self.sys_exit()
    }
    pub async fn sys_sched_yield(&mut self) -> SysRet {
        stack_trace!();
        thread::yield_now().await;
        Ok(0)
    }
    pub async fn sys_nanosleep(&mut self) -> SysRet {
        stack_trace!();
        let (req, rem): (UserReadPtr<TimeSpec>, UserWritePtr<TimeSpec>) = self.cx.into();
        if req.is_null() {
            return Err(SysError::EINVAL);
        }
        let req = UserCheck::new(self.process)
            .readonly_value(req)
            .await?
            .load();
        req.valid()?;
        let rem = match rem.nonnull_mut() {
            None => None,
            Some(rem) => Some(UserCheck::new(self.process).writable_value(rem).await?),
        };
        let now = timer::now();
        let dur = req.as_duration();
        let deadline = now + dur;
        let ret = timer::sleep::sleep(dur, &self.process.event_bus).await;
        if let Some(rem) = rem {
            let time_end = timer::now();
            let time_rem = if time_end < deadline {
                deadline - time_end
            } else {
                Duration::ZERO
            };
            rem.store(TimeSpec::from_duration(time_rem));
        }
        ret
    }
    pub fn sys_brk(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_brk");
        }
        let brk: usize = self.cx.para1();
        // println!("sys_brk: {:#x}", brk);
        let brk = if brk == 0 {
            self.alive_then(|a| a.user_space.get_brk())
        } else {
            let brk = UserAddr::try_from(brk as *const u8)?;
            if let Some(asid) =
                self.alive_then(|a| a.user_space.reset_brk(brk, &mut frame::default_allocator()))?
            {
                local::all_hart_sfence_vma_asid(asid);
            }
            brk
        };
        // println!("    -> {:#x}", brk);
        Ok(brk.into_usize())
    }
    pub async fn sys_uname(&mut self) -> SysRet {
        stack_trace!();

        if PRINT_SYSCALL_PROCESS {
            println!("sys_uname");
        }
        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Utsname {
            sysname: [u8; 65],
            nodename: [u8; 65],
            release: [u8; 65],
            version: [u8; 65],
            machine: [u8; 65],
            domainname: [u8; 65],
        }

        let buf: UserWritePtr<Utsname> = self.cx.para1();
        let buf = UserCheck::new(self.process).writable_value(buf).await?;
        let mut access = buf.access_mut();
        let uts_name = &mut access[0];
        *uts_name = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        macro_rules! xwrite {
            ($name: ident, $str: expr) => {
                let v = $str;
                uts_name.$name[..v.len()].copy_from_slice(v);
            };
        }
        xwrite!(sysname, b"Linux");
        xwrite!(nodename, b"FTL-OS");
        xwrite!(release, b"5.0.0");
        xwrite!(version, b"1.0.0");
        xwrite!(machine, b"riscv64");
        xwrite!(domainname, b"192.168.0.1");
        Ok(0)
    }
    pub fn sys_umask(&mut self) -> SysRet {
        let _umask: u32 = self.cx.para1();
        Ok(0o777)
    }
}
