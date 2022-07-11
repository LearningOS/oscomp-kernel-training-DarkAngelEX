use alloc::boxed::Box;

use super::{AsyncFile, File};
use crate::{console, sync::SleepMutex};

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
    fn read<'a>(&'a self, buf: &'a mut [u8]) -> AsyncFile {
        Box::pin(async move {
            const PRINT_STDIN: bool = false;
            let len = buf.len();
            for i in 0..len {
                let mut c: usize;
                if PRINT_STDIN {
                    print!("?");
                }
                loop {
                    c = console::getchar() as usize;
                    if [0, u32::MAX as usize].contains(&c) {
                        if !crate::xdebug::CLOSE_TIME_INTERRUPT {
                            use crate::timer::{sleep, TimeTicks};
                            sleep::just_wait(TimeTicks::from_millisecond(5)).await;
                        } else {
                            crate::process::thread::yield_now().await;
                        }
                        continue;
                    }
                    break;
                }
                if PRINT_STDIN {
                    print!("!");
                }
                buf[i] = c as u8;
            }
            Ok(len)
        })
    }
    fn write<'a>(&'a self, _buf: &'a [u8]) -> AsyncFile {
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
    fn read<'a>(&'a self, _buf: &'a mut [u8]) -> AsyncFile {
        panic!("Cannot read from stdout!");
    }
    fn write<'a>(&'a self, buf: &'a [u8]) -> AsyncFile {
        Box::pin(async move {
            use core::str::lossy;
            let lock = STDOUT_MUTEX.lock().await;
            let str = buf;
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
