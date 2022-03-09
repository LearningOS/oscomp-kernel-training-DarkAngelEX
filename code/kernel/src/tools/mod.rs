#![allow(dead_code)]

pub mod allocator;
pub mod container;

pub mod error;

pub const fn bool_result(x: bool) -> Result<(), ()> {
    if x {
        Ok(())
    } else {
        Err(())
    }
}

#[macro_export]
macro_rules! impl_usize_from {
    ($name: ident, $v: ident, $body: stmt) => {
        impl From<$name> for usize {
            fn from($v: $name) -> Self {
                $body
            }
        }
        impl $name {
            pub const fn into_usize(&self) -> usize {
                let $v = self;
                $body
            }
        }
    };
}

pub struct FailRun<T: FnOnce()> {
    drop_run: Option<T>,
}

impl<T: FnOnce()> Drop for FailRun<T> {
    fn drop(&mut self) {
        if let Some(f) = self.drop_run.take() {
            f()
        }
    }
}

impl<T: FnOnce()> FailRun<T> {
    pub fn new(f: T) -> Self {
        Self { drop_run: Some(f) }
    }
    pub fn consume(mut self) {
        self.drop_run = None;
    }
}

pub trait Wrapper<T> {
    type Output;
    fn wrapper(a: T) -> Self::Output;
}

#[derive(Clone)]
pub struct ForwardWrapper;
impl<T> Wrapper<T> for ForwardWrapper {
    type Output = T;
    fn wrapper(a: T) -> T {
        a
    }
}

pub fn size_to_mkb(size: usize) -> (usize, usize, usize) {
    let mask = 1 << 10;
    (size >> 20, (size >> 10) % mask, size % mask)
}

pub fn next_instruction_sepc(sepc: usize, ir: u8) -> usize {
    if ir & 0b11 == 0b11 {
        sepc + 4
    } else {
        sepc + 2 //  RVC extend: Compressed Instructions
    }
}

pub fn next_sepc(sepc: usize) -> usize {
    let ir = unsafe { *(sepc as *const u8) };
    next_instruction_sepc(sepc, ir)
}
