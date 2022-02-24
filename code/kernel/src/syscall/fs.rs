use crate::{
    console,
    memory::user_ptr::{UserInPtr, UserOutPtr},
    process::thread,
    syscall::SysError,
    user,
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
        let (fd, vaild_data, len) = {
            let (fd, buf, len): (usize, *mut u8, usize) = self.cx.parameter3();
            let guard = self.using_space()?;
            let vaild_data = user::translated_user_writable_slice(buf, len, guard.access())?;
            (fd, vaild_data, len)
        };
        if fd == FD_STDIN {
            let guard = self.using_space()?;
            for ch in vaild_data.access_mut(guard.access()).iter_mut() {
                *ch = console::getchar() as u8;
            }
        }
        Ok(len)
    }
    pub async fn sys_write(&mut self) -> SysResult {
        if PRINT_SYSCALL_FS {
            println!("sys_write");
        }
        let (fd, vaild_data, len) = {
            let (fd, buf, len): (usize, *const u8, usize) = self.cx.parameter3();
            let guard = self.using_space()?;
            let vaild_data = user::translated_user_readonly_slice(buf, len, guard.access())?;
            (fd, vaild_data, len)
        };
        if fd == FD_STDOUT {
            loop {
                {
                    if let Some(lock) = console::stdout_try_lock() {
                        let guard = self.using_space()?;
                        let a = vaild_data.access(guard.access());
                        let str = core::str::from_utf8(&*a).map_err(|_| SysError::EFAULT)?;
                        print_unlock!("{}", str);
                        return Ok(len);
                    }
                }
                thread::yield_now().await;
            }
        } else {
            unimplemented!()
        }
        Ok(len)
    }
}
