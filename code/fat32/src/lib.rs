#![feature(new_uninit)]
#![feature(const_btree_new)]
#![feature(const_fn_trait_bound)]
#![feature(const_trait_impl)]
#![feature(negative_impls)]
#![feature(nonzero_ops)]

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
mod cache;
mod manager;
mod mutex;
mod sleep_mutex;

pub use block_dev::{AsyncRet, BlockDevice};
