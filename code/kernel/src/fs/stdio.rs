use alloc::{boxed::Box, sync::Arc};

use super::{AsyncFileOutput, File};
use crate::{
    console,
    process::{thread, Process},
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
    fn read(self: Arc<Self>, proc: Arc<Process>, buf: UserDataMut<u8>) -> AsyncFileOutput {
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
                let guard = proc.using_space().unwrap();
                buf.access_mut(&guard)[i] = ch;
            }
            Ok(len)
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
            // print!("!");
            let lock = STDOUT_MUTEX.lock().await;
            // print!("<");
            let guard = proc.using_space().unwrap();
            let str = buf.access(&guard);
            print_unlock!("{}", unsafe { core::str::from_utf8_unchecked(&*str) });
            let len = buf.len();
            drop(lock);
            // print!(">");
            Ok(len)
        })
    }
}
