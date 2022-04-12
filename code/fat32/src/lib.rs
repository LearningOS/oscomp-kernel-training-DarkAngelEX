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
#![feature(type_alias_impl_trait)]
#![feature(async_iterator)]
#![feature(generic_associated_types)]
#![feature(try_trait_v2)]
#![feature(control_flow_enum)]
#![feature(try_blocks)]

#[macro_use]
extern crate bitflags;
extern crate alloc;

#[macro_use]
mod console;
#[macro_use]
mod xdebug;
pub mod access;
mod block_cache;
mod block_dev;
pub mod block_sync;
mod fat_list;
mod layout;
mod manager;
mod mutex;
mod sleep_mutex;
mod tools;
pub mod xerror;
pub mod xtest;
pub mod inode;
pub use block_dev::{AsyncRet, BlockDevice};
pub use manager::Fat32Manager;

pub trait FsSystem {
    fn new(max_cache: usize) -> Self;
}
