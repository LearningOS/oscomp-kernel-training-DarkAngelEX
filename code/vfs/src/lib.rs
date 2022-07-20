//!
//! 运行在内存的文件系统, 关机后数据全部删除
//!
#![no_std]
#![feature(map_try_insert)]
#![feature(generic_arg_infer)]
#![feature(const_btree_new)]
#![feature(build_hasher_simple_hash_one)]
#![feature(bool_to_option)]
#![feature(trait_alias)]

extern crate alloc;
#[macro_use]
extern crate ftl_util;
#[macro_use]
extern crate bitflags;

pub use {
    file::{File, VfsFile},
    manager::VfsManager,
};

mod dentry;
mod file;
mod hash_name;
mod inode;
mod manager;
mod mount;
mod spfs;
mod test;
