//!
//! 运行在内存的文件系统, 关机后数据全部删除
//!
#![no_std]
#![feature(map_try_insert)]
#![feature(generic_arg_infer)]
#![feature(const_btree_new)]
#![feature(build_hasher_simple_hash_one)]
#![feature(bool_to_option)]
#![feature(sync_unsafe_cell)]
#![feature(trait_alias)]
#![feature(get_mut_unchecked)]
#![feature(receiver_trait)]
#![allow(dead_code)]

const PRINT_OP: bool = true;
const PRINT_INTO_LRU: bool = true;

extern crate alloc;
#[macro_use]
extern crate ftl_util;
// #[macro_use]
// extern crate bitflags;

pub use {
    file::{File, VfsFile},
    manager::VfsManager,
};

mod dentry;
mod file;
mod fssp;
mod hash_name;
mod inode;
mod manager;
mod mount;
#[cfg(test)]
mod test;
pub mod tmpfs;
