#![feature(new_uninit)]
#![feature(const_btree_new)]

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

pub use block_dev::{AsyncRet, BlockDevice};
