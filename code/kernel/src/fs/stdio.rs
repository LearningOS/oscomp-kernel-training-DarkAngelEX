use alloc::{boxed::Box, sync::Arc};

use super::{AsyncFile, File};
use crate::{
    console,
    process::thread,
    sync::sleep_mutex::SleepMutex,
    user::{UserData, UserDataMut},
};

pub struct Stdin;

pub struct Stdout;

static STDOUT_MUTEX: SleepMutex<()> = SleepMutex::new(());

impl File for Stdin {
    fn readable(&self) -> bool {
        true
    }
    fn writable(&self) -> bool {
        false
    }
    fn can_mmap(&self) -> bool {
        false
    }
    fn read(self: Arc<Self>, buf: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            let len = buf.len();
            for i in 0..len {
                let mut c: usize;
                loop {
                    c = console::getchar() as usize;
                    if c == 0 {
                        // suspend_current_and_run_next();
                        thread::yield_now().await;
                        continue;
                    }
                    break;
                }
                let ch = c as u8;
                buf.access_mut()[i] = ch;
            }
            Ok(len)
        })
    }
    fn write(self: Arc<Self>, _buf: UserData<u8>) -> AsyncFile {
        panic!("Cannot write to stdin!");
    }
}

impl File for Stdout {
    fn readable(&self) -> bool {
        false
    }
    fn writable(&self) -> bool {
        true
    }
    fn read(self: Arc<Self>, _buf: UserDataMut<u8>) -> AsyncFile {
        panic!("Cannot read from stdout!");
    }
    fn write(self: Arc<Self>, buf: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            // print!("!");
            let lock = STDOUT_MUTEX.lock().await;
            // print!("<");
            let str = buf.access();
            print_unlock!("{}", unsafe { core::str::from_utf8_unchecked(&*str) });
            let len = buf.len();
            drop(lock);
            // print!(">");
            Ok(len)
        })
    }
}
