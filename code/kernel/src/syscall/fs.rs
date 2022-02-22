use riscv::register::sstatus;

use crate::{
    console,
    memory::user_ptr::{UserInPtr, UserOutPtr},
    process::thread,
    syscall::SysError,
    user::{self, AutoSie},
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::{SysResult, Syscall};

const PRINT_SYSCALL_FS: bool = false && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

impl<'a> Syscall<'a> {
    pub async fn sys_read(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_read");
        }
        let (fd, buf, len): (usize, UserOutPtr<u8>, usize) = self.cx.parameter3();
        if fd == FD_STDIN {
            let guard = match self.process.using_space() {
                Ok(a) => a,
                Err(()) => {
                    self.do_exit = true;
                    return Err(SysError::ESRCH);
                }
            };
            let x = user::translated_user_writable_slice(buf.raw_ptr_mut(), len, guard.access())?;
            for ch in x.access_mut(guard.access()).iter_mut() {
                *ch = console::getchar() as u8;
            }
        }
        Ok(len)
    }
    pub async fn sys_write(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_write");
        }
        let (fd, buf, len): (usize, UserInPtr<u8>, usize) = self.cx.parameter3();
        if fd == FD_STDOUT {
            let vaild_data = {
                let guard = match self.process.using_space() {
                    Ok(a) => a,
                    Err(()) => {
                        self.do_exit = true;
                        return Err(SysError::ESRCH);
                    }
                };
                user::translated_user_readonly_slice(buf.raw_ptr(), len, guard.access())?
            };
            let lock = loop {
                if let Some(lock) = console::stdout_try_lock() {
                    break lock;
                } else {
                    thread::yield_now().await;
                }
            };
            let guard = match self.process.using_space() {
                Ok(a) => a,
                Err(()) => {
                    self.do_exit = true;
                    return Err(SysError::ESRCH);
                }
            };
            let a = vaild_data.access(guard.access());
            let str = core::str::from_utf8(&*a).map_err(|_| SysError::EFAULT)?;
            print_unlock!("{}", str);
            drop(lock);
        } else {
            unimplemented!()
        }
        Ok(len)
    }
}
