#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(const_slice_from_raw_parts)]
#![feature(alloc_error_handler)]
#![feature(const_fn_trait_bound)]
#![feature(default_free_fn)]
#![feature(bench_black_box)]
#![feature(split_array)]
#![feature(bool_to_option)]
#![feature(asm_const)]
#![feature(trait_alias)]
#![feature(const_btree_new)]
#![feature(map_first_last)]
#![feature(never_type)]
#![feature(slice_pattern)]
#![feature(map_try_insert)]

extern crate alloc;
extern crate async_task;
#[macro_use]
extern crate bitflags;
extern crate riscv;
// extern crate lazy_static;
extern crate xmas_elf;

use core::arch::global_asm;

mod config;

#[macro_use]
mod console;
#[macro_use]
mod xdebug;
mod executor;
mod fdt;
mod hart;
mod local;
mod lang_items;
mod loader;
mod memory;
// mod message;
mod process;
mod sync;
mod syscall;
// mod task;
mod timer;
mod tools;
mod trap;
mod user;

global_asm!(include_str!("link_app.S"));

///
/// This function will be called by rust_main() in hart/mod.rs
///
/// It will run on each core.
///
pub fn kmain(hart_id: usize) -> ! {
    loop {
        executor::run_until_idle();
    }
}
