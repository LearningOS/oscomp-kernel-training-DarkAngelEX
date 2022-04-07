#![no_std]

extern crate alloc;

#[macro_use]
mod console;
#[macro_use]
mod xdebug;
mod block_dev;
mod fat_list;
mod layout;
pub mod test;
mod tools;

pub use block_dev::{AsyncRet, LogicBlockDevice};

pub const BLOCK_SZ: usize = 512;
