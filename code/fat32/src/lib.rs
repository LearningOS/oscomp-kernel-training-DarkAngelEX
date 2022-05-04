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
#![feature(int_roundings)]
#![feature(get_mut_unchecked)]
#![feature(split_array)]

const PRINT_BLOCK_OP: bool = false;
const PRINT_INODE_OP: bool = false;

#[macro_use]
extern crate ftl_util;
#[macro_use]
extern crate bitflags;
extern crate alloc;

mod block;
mod block_dev;
mod fat_list;
mod layout;
mod manager;
mod mutex;

mod inode;
mod tools;
pub mod xtest;

pub use block_dev::AsyncRet;
pub use ftl_util::{console_init, debug_init, device::BlockDevice, utc_time::UtcTime};
pub use inode::{dir_inode::DirInode, file_inode::FileInode, AnyInode};
pub use layout::name::Attr;
pub use manager::Fat32Manager;

pub trait FsSystem {
    fn new(max_cache: usize) -> Self;
}
