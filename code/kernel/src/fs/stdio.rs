use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::{AsyncFileOutput, File};
use crate::hart::sbi::console_getchar;
use crate::process::{thread, Process};
use crate::sync::sleep_mutex::SleepMutex;
use crate::user::{UserData, UserDataMut};

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
    fn read(self: Arc<Self>, proc: Arc<Process>, buf: UserDataMut<u8>) -> AsyncFileOutput {
        assert_eq!(buf.len(), 1);
        // busy loop
        Box::pin(async move {
            let mut c: usize;
            loop {
                c = console_getchar();
                if c == 0 {
                    // suspend_current_and_run_next();
                    thread::yield_now().await;
                    continue;
                } else {
                    break;
                }
            }
            let ch = c as u8;
            let guard = proc.using_space().unwrap();
            buf.access_mut(&guard)[0] = ch;
            Ok(1)
        })
    }
    fn write(self: Arc<Self>, _proc: Arc<Process>, _buf: UserData<u8>) -> AsyncFileOutput {
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
    fn read(self: Arc<Self>, _proc: Arc<Process>, _buf: UserDataMut<u8>) -> AsyncFileOutput {
        panic!("Cannot read from stdout!");
    }
    fn write(self: Arc<Self>, proc: Arc<Process>, buf: UserData<u8>) -> AsyncFileOutput {
        Box::pin(async move {
            let _lock = STDOUT_MUTEX.lock().await;
            let guard = proc.using_space().unwrap();
            let str = buf.access(&guard);
            print!("{}", core::str::from_utf8(&*str).unwrap());
            let len = buf.len();
            Ok(len)
        })
    }
}
