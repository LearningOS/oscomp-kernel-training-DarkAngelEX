use core::{convert::TryFrom, ops::Deref, sync::atomic::Ordering};

use alloc::{string::String, vec::Vec};

use crate::{
    config::{PAGE_SIZE, USER_STACK_RESERVE},
    fs::{self, Mode},
    local,
    memory::{
        self,
        address::{PageCount, UserAddr},
        asid::USING_ASID,
        user_ptr::{UserInOutPtr, UserReadPtr, UserWritePtr},
        UserSpace,
    },
    process::{
        resource::{self, RLimit},
        search, thread, userloop, CloneFlag, Pid,
    },
    sync::even_bus::{self, Event},
    timer::{self, sleep::SleepFuture, TimeSpec, TimeTicks},
    tools::allocator::from_usize_allocator::FromUsize,
    user::check::UserCheck,
    xdebug::{NeverFail, PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysError, SysResult, Syscall};

const PRINT_SYSCALL_PROCESS: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub async fn sys_clone(&mut self) -> SysResult {
        stack_trace!();
        let (flag, new_sp, ptid, tls, ctid): (
            usize,
            usize,
            UserInOutPtr<u32>,
            UserInOutPtr<u8>,
            UserInOutPtr<u32>,
        ) = self.cx.into();
        const PRINT_THIS: bool = true;
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
        let set_child_tid = if flag.contains(CloneFlag::CLONE_CHILD_SETTID) {
            ctid
        } else {
            UserInOutPtr::null()
        };
        let clear_child_tid = if flag.contains(CloneFlag::CLONE_CHILD_CLEARTID) {
            ctid
        } else {
            UserInOutPtr::null()
        };
        let tls = if flag.contains(CloneFlag::CLONE_SETTLS) {
            tls
        } else {
            UserInOutPtr::null()
        };
        let exit_signal = if !flag.contains(CloneFlag::CLONE_DETACHED) {
            (flag & CloneFlag::EXIT_SIGNAL).bits() as u32
        } else {
            0
        };
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
            match UserCheck::new(self.process)
                .translated_user_writable_value(ptid)
                .await
            {
                Ok(ptid) => ptid.store(new.tid().0 as u32),
                Err(_e) => {
                    println!("sys_clone ignore error: CLONE_PARENT_SETTID fail");
                }
            }
        }
        userloop::spawn(new);
        if PRINT_SYSCALL_PROCESS || PRINT_THIS {
            println!("\t-> {:?}", tid);
        }
        Ok(tid.0)
    }
    pub async fn sys_execve(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_execve {:?}", self.process.pid());
        }
        let (path, args, envp): (
            UserReadPtr<u8>,
            UserReadPtr<UserReadPtr<u8>>,
            UserReadPtr<UserReadPtr<u8>>,
        ) = self.cx.para3();
        let user_check = UserCheck::new(self.process);
        let path = String::from_utf8(
            user_check
                .translated_user_array_zero_end(path)
                .await?
                .to_vec(),
        )?;
        stack_trace!("sys_execve path: {}", path);
        let args: Vec<String> = user_check
            .translated_user_2d_array_zero_end(args)
            .await?
            .into_iter()
            .map(|a| unsafe { String::from_utf8_unchecked(a.to_vec()) })
            .collect();
        let envp = user_check
            .translated_user_2d_array_zero_end(envp)
            .await?
            .into_iter()
            .map(|a| unsafe { String::from_utf8_unchecked(a.to_vec()) })
            .collect::<Vec<String>>();

        let args_size = UserSpace::push_args_size(&args, &envp);
        let stack_reverse = args_size + PageCount(USER_STACK_RESERVE / PAGE_SIZE);
        let inode = fs::open_file(
            Some(Ok(self.alive_then(|a| a.cwd.clone())?.path_iter())),
            path.as_str(),
            fs::OpenFlags::RDONLY,
            Mode(0o500),
        )
        .await?;
        let mut iter = inode.path_iter();
        iter.next_back();
        let dir = fs::open_file(Some(Ok(iter)), "", fs::OpenFlags::RDONLY, Mode(0o500)).await?;
        let elf_data = inode.read_all().await?;
        let (user_space, user_sp, entry_point, auxv) =
            UserSpace::from_elf(elf_data.as_slice(), stack_reverse)
                .map_err(|_e| SysError::ENOEXEC)?;

        // TODO: kill other thread and await
        let mut alive = self.alive_lock()?;
        if alive.threads.len() > 1 {
            todo!();
        }
        let check = NeverFail::new();
        if !USING_ASID {
            local::all_hart_sfence_vma_asid(alive.asid());
        }
        unsafe { user_space.using() };
        let (user_sp, argc, argv, envp) =
            user_space.push_args(user_sp.into(), &args, &alive.envp, &auxv, args_size);
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
        cx.exec_init(user_sp, entry_point, sstatus, fcsr, argc, argv, envp);
        local::all_hart_fence_i();
        check.assume_success();
        // rtld_fini: 动态链接器析构函数
        // 如果这个值非零, glibc会在程序结束时把它当作函数指针并运行
        let rtld_fini = 0;
        Ok(rtld_fini)
    }
    pub async fn sys_wait4(&mut self) -> SysResult {
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
                    println!("[FTL OS]wait4 fail: no child");
                    return Err(SysError::ECHILD);
                }
                p
            };
            if let Some(process) = process {
                if let Some(exit_code_ptr) = exit_code_ptr.nonnull_mut() {
                    let exit_code = process.exit_code.load(Ordering::Relaxed);
                    let access = UserCheck::new(self.process)
                        .translated_user_writable_value(exit_code_ptr)
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
            if let Err(_e) = even_bus::wait_for_event(event_bus, Event::CHILD_PROCESS_QUIT)
                .await
                .and_then(|_x| event_bus.clear(Event::CHILD_PROCESS_QUIT))
            {
                if PRINT_SYSCALL_PROCESS {
                    println!("sys_wait4 fail by close {:?}", this_pid);
                }
                self.do_exit = true;
                return Err(SysError::ESRCH);
            }
            // check then
        }
    }
    pub fn sys_set_tid_address(&mut self) -> SysResult {
        stack_trace!();
        let clear_child_tid: UserInOutPtr<u32> = self.cx.para1();
        self.thread.inner().clear_child_tid = clear_child_tid;
        Ok(self.thread.tid().0)
    }
    /// 设置pgid
    ///
    /// 如果pid为0则处理本进程, 如果pgid为0则设置为pid
    pub fn sys_setpgid(&mut self) -> SysResult {
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
    pub fn sys_getpgid(&mut self) -> SysResult {
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
        return Ok(pid);
    }
    pub fn sys_getpid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getpid -> {:?}", self.process.pid());
        }
        Ok(self.process.pid().0)
    }
    pub fn sys_getppid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getpid");
        }
        let pid = self
            .alive_then(|a| {
                a.parent
                    .as_ref()
                    .and_then(|p| p.upgrade())
                    .map(|p| p.pid().0)
            })?
            .unwrap_or(0); // initproc
        Ok(pid)
    }
    pub fn sys_getuid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getuid");
        }
        Ok(0)
    }
    pub fn sys_geteuid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_geteuid");
        }
        Ok(0)
    }
    pub fn sys_getegid(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_ALL {
            println!("sys_getegid");
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
        self.process.event_bus.close();
        let mut lock = self.process.alive.lock();
        let alive = match lock.as_mut() {
            Some(a) => a,
            None => return Err(SysError::ESRCH),
        };
        self.process.exit_code.store(exit_code, Ordering::Relaxed);
        // TODO: waiting other thread exit
        memory::set_satp_by_global();
        alive.clear_all(self.process.pid());
        if let Some(sig) = self.thread.exit_send_signal() {
            alive.parent.as_ref().and_then(|a| a.upgrade()).map(|a| {
                a.signal_manager.receive(sig);
                let _ = a.event_bus.set(Event::RECEIVE_SIGNAL);
            });
        }
        *lock = None;
        Ok(0)
    }
    pub fn sys_exit_group(&mut self) -> SysResult {
        stack_trace!();
        self.sys_exit()
    }
    pub async fn sys_sched_yield(&mut self) -> SysResult {
        stack_trace!();
        thread::yield_now().await;
        Ok(0)
    }
    pub async fn sys_nanosleep(&mut self) -> SysResult {
        stack_trace!();
        let (req, rem): (UserReadPtr<TimeSpec>, UserWritePtr<TimeSpec>) = self.cx.into();
        if req.is_null() {
            return Err(SysError::EINVAL);
        }
        let req = UserCheck::new(self.process)
            .translated_user_readonly_value(req)
            .await?
            .load();
        req.valid()?;
        let rem = match rem.nonnull_mut() {
            None => None,
            Some(rem) => Some(
                UserCheck::new(self.process)
                    .translated_user_writable_value(rem)
                    .await?,
            ),
        };
        let deadline = timer::get_time_ticks() + TimeTicks::from_time_spec(req);
        let ret = SleepFuture::new(deadline, self.process.event_bus.clone()).await;
        if let Some(rem) = rem {
            let time_end = timer::get_time_ticks();
            let time_rem = if time_end < deadline {
                deadline - time_end
            } else {
                TimeTicks::ZERO
            };
            rem.store(time_rem.time_sepc());
        }
        ret
    }
    pub fn sys_brk(&mut self) -> SysResult {
        stack_trace!();
        if PRINT_SYSCALL_PROCESS {
            println!("sys_brk");
        }
        let brk: usize = self.cx.para1();
        // println!("sys_brk: {:#x}", brk);
        let brk = if brk == 0 {
            self.alive_then(|a| a.user_space.get_brk())?
        } else {
            let brk = UserAddr::try_from(brk as *const u8)?;
            self.alive_then(|a| a.user_space.reset_brk(brk))??;
            brk
        };
        // println!("    -> {:#x}", brk);
        Ok(brk.into_usize())
    }
    pub async fn sys_uname(&mut self) -> SysResult {
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
        let buf = UserCheck::new(self.process)
            .translated_user_writable_value(buf)
            .await?;
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
    /// 设置系统资源
    pub async fn sys_prlimit64(&mut self) -> SysResult {
        stack_trace!();
        let (pid, resource, new_limit, old_limit): (
            Pid,
            u32,
            UserReadPtr<RLimit>,
            UserWritePtr<RLimit>,
        ) = self.cx.into();

        if PRINT_SYSCALL_PROCESS {
            println!(
                "sys_prlimit64 pid:{:?}, resource:{}, new_ptr: {:#x}, old_ptr: {:#x}",
                pid,
                resource,
                new_limit.as_usize(),
                old_limit.as_usize()
            );
        }

        let process = search::find_proc(pid).ok_or(SysError::ESRCH)?;
        let uc = UserCheck::new(self.process);
        let new = match new_limit.is_null() {
            false => Some(uc.translated_user_readonly_value(new_limit).await?.load()),
            true => None,
        };
        if (PRINT_SYSCALL_PROCESS || false) && let Some(new) = new {
            println!("new: {:?}", new);
        }
        let old = resource::prlimit_impl(&process, resource, new)?;
        if !old_limit.is_null() {
            uc.translated_user_writable_value(old_limit)
                .await?
                .store(old);
        }
        Ok(0)
    }
}
