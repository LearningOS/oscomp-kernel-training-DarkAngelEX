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

extern crate alloc;
extern crate bitflags;
extern crate lazy_static;
extern crate xmas_elf;

use core::arch::global_asm;

mod config;
#[macro_use]
mod console;
#[macro_use]
mod debug;
mod fdt;
mod lang_items;
mod loader;
mod memory;
mod riscv;
mod scheduler;
mod sync;
mod syscall;
mod task;
pub mod tools;
mod trap;

global_asm!(include_str!("link_app.S"));

///
/// This function will be called by rust_main() in riscv/mod.rs
///
/// It will run on each core.
///
pub fn kmain() -> ! {
    loop {}
}
