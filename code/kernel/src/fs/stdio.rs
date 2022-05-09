use alloc::{borrow::Cow, boxed::Box, string::String};

use super::{AsyncFile, File};
use crate::{
    console,
    process::thread,
    sync::SleepMutex,
    user::{UserData, UserDataMut},
};

pub struct Stdin;

pub struct Stdout;

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
    fn read(&self, buf: UserDataMut<u8>) -> AsyncFile {
        Box::pin(async move {
            let len = buf.len();
            for i in 0..len {
                let mut c: usize;
                loop {
                    c = console::getchar() as usize;
                    if c == 0 {
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
    fn write(&self, _buf: UserData<u8>) -> AsyncFile {
        panic!("Cannot write to stdin!");
    }
}

static STDOUT_MUTEX: SleepMutex<()> = SleepMutex::new(());

impl File for Stdout {
    fn readable(&self) -> bool {
        false
    }
    fn writable(&self) -> bool {
        true
    }
    fn read(&self, _buf: UserDataMut<u8>) -> AsyncFile {
        panic!("Cannot read from stdout!");
    }
    fn write(&self, buf: UserData<u8>) -> AsyncFile {
        Box::pin(async move {
            use core::str::lossy;
            let lock = STDOUT_MUTEX.lock().await;
            let str = buf.access();
            let iter = lossy::Utf8Lossy::from_bytes(&*str).chunks();
            for lossy::Utf8LossyChunk { valid, broken } in iter {
                if !valid.is_empty() {
                    print_unlocked!("{}", valid);
                }
                if !broken.is_empty() {
                    print_unlocked!("{}", core::char::REPLACEMENT_CHARACTER);
                }
            }
            drop(lock);
            Ok(buf.len())
        })
    }
}
