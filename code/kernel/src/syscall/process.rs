use core::sync::atomic::Ordering;

use alloc::string::String;

use crate::{
    hart::sfence,
    loader,
    memory::{allocator::frame, user_ptr::UserOutPtr, UserSpace},
    process::{userloop, Pid},
    sync::even_bus::{self, Event},
    tools::allocator::from_usize_allocator::FromUsize,
    user,
    xdebug::{NeverFail, PRINT_SYSCALL},
};

use super::{SysError, SysResult, Syscall};

impl<'a> Syscall<'a> {
    pub fn sys_fork(&mut self) -> SysResult {
        if PRINT_SYSCALL {
            println!("sys_fork");
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
        Ok(pid.into_usize())
    }
    pub fn sys_exec(&mut self) -> SysResult {
        if PRINT_SYSCALL {
            println!("sys_exec");
        }
        let (path, argv, envp): (*const u8, *const *const u8, *const *const u8) =
            self.cx.parameter3();
        let path = String::from_utf8(user::translated_user_array_zero_end(path)?.into_vec())
            .map_err(|_| SysError::EFAULT)?;
        // let argv = user::translated_user_2d_array_zero_end(argv)?;
        // let envp = user::translated_user_2d_array_zero_end(envp)?;

        let mut lock = self.process.alive.lock(place!());
        let alive = match lock.as_mut() {
            Some(a) => a,
            None => {
                *self.do_exit = true;
                return Err(SysError::ESRCH);
            }
        };
        let allocator = &mut frame::defualt_allocator();
        // TODO kill other thread and await
        if alive.threads.len() > 1 {
            todo!();
        }
        let elf_data = loader::get_app_data_by_name(path.as_str()).ok_or(SysError::ENFILE)?;
        let (mut user_space, stack_id, user_sp, entry_point) =
            UserSpace::from_elf(elf_data, allocator).map_err(|_| SysError::ENOEXEC)?;
        let check = NeverFail::new();
        // reset stack_id
        user_space.using();
        sfence::fence_i();
        alive.exec_path = path;
        alive.user_space = user_space;
        self.thread.inner().stack_id = stack_id;
        let (argc, argv) = (0, 0);
        let sstatus = self.thread.get_context().user_sstatus;
        self.thread
            .get_context()
            .exec_init(user_sp, entry_point, sstatus, argc, argv);

        check.assume_success();
        Ok(argc)
    }
    pub async fn sys_waitpid(&mut self) -> SysResult {
        if PRINT_SYSCALL {
            println!("sys_waitpid");
        }
        let (pid, exit_code_ptr): (isize, UserOutPtr<i32>) = self.cx.parameter2();
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
            // zomebies process.
            let mut xlock = self.process.alive.lock(place!());
            let p = match xlock.as_mut() {
                Some(p) => p,
                None => {
                    *self.do_exit = true;
                    return Err(SysError::ESRCH);
                }
            };
            let process = match target {
                WaitFor::AnyChild => p.children.try_remove_zombie_any(),
                WaitFor::Pid(pid) => p.children.try_remove_zombie(pid),
                WaitFor::PGid(_) => unimplemented!(),
                WaitFor::AnyChildInGroup => unimplemented!(),
            };
            if let Some(process) = process {
                if let Some(exit_code_ptr) = exit_code_ptr.nonnull_mut() {
                    p.user_space.using();
                    let exit_code = process.exit_code.load(Ordering::Relaxed);
                    let access = user::translated_user_writable_slice(exit_code_ptr as *mut u8, 4)?;
                    let exit_code_slice =
                        core::ptr::slice_from_raw_parts(&exit_code as *const _ as *const u8, 4);
                    access
                        .access_mut()
                        .copy_from_slice(unsafe { &*exit_code_slice });
                }
                return Ok(process.pid().into_usize());
            }
            drop(xlock);
            let event_bus = &self.process.event_bus;
            if let Err(_e) =
                even_bus::wait_for_event(event_bus.clone(), Event::CHILD_PROCESS_QUIT).await
            {
                *self.do_exit = true;
                return Err(SysError::ESRCH);
            }
            if let Err(_e) = event_bus.lock(place!()).clear(Event::CHILD_PROCESS_QUIT) {
                *self.do_exit = true;
                return Err(SysError::ESRCH);
            }
            // check then
        }
    }
}
