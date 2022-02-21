use core::fmt::Write;

use crate::{
    console,
    memory::user_ptr::{UserInPtr, UserOutPtr},
    process::thread,
    syscall::SysError,
    user,
    xdebug::PRINT_SYSCALL,
};

use super::{SysResult, Syscall};

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

impl<'a> Syscall<'a> {
    pub async fn sys_read(&mut self) -> SysResult {
        if PRINT_SYSCALL {
            println!("sys_read");
        }
        let (fd, buf, len): (usize, UserOutPtr<u8>, usize) = self.cx.parameter3();
        if fd == FD_STDIN {
            let x = user::translated_user_writable_slice(buf.raw_ptr_mut(), len)?;
            for ch in x.access_mut().iter_mut() {
                *ch = console::getchar() as u8;
            }
        }
        Ok(len)
    }
    pub async fn sys_write(&mut self) -> SysResult {
        if PRINT_SYSCALL {
            println!("sys_write");
        }
        let (fd, buf, len): (usize, UserInPtr<u8>, usize) = self.cx.parameter3();
        if fd == FD_STDOUT {
            let x = user::translated_user_readonly_slice(buf.raw_ptr(), len)?;
            let a = x.access();
            let str = core::str::from_utf8(&*a).map_err(|_| SysError::EFAULT)?;
            let lock = loop {
                if let Some(lock) = console::stdout_try_lock() {
                    break lock;
                } else {
                    thread::yield_now().await;
                }
            };
            print_unlock!("{}", str);
            drop(lock);
        } else {
            unimplemented!()
        }
        Ok(len)
    }
}
