//!
//! 运行在内存的文件系统, 关机后数据全部删除
//!
#![no_std]
#![feature(map_try_insert)]

extern crate alloc;
#[macro_use]
extern crate ftl_util;
#[macro_use]
extern crate bitflags;

mod inode;
mod manager;
