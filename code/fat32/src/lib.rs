#![no_std]
#![feature(new_uninit)]
#![feature(allocator_api)]
#![feature(const_btree_new)]
#![feature(const_fn_trait_bound)]
#![feature(const_trait_impl)]
#![feature(negative_impls)]
#![feature(nonzero_ops)]
#![feature(exclusive_range_pattern)]
#![feature(map_try_insert)]
#![feature(bool_to_option)]
#![feature(const_maybe_uninit_zeroed)]
#![feature(int_log)]
#![feature(map_first_last)]

extern crate alloc;

#[macro_use]
mod console;
#[macro_use]
mod xdebug;
mod block_cache;
mod block_dev;
pub mod block_sync;
mod fat_list;
mod layout;
mod manager;
mod mutex;
mod sleep_mutex;
mod tools;
pub mod xtest;
pub use block_dev::{AsyncRet, BlockDevice};
pub use manager::Fat32Manager;

pub trait FsSystem {
    fn new(max_cache: usize) -> Self;
}
