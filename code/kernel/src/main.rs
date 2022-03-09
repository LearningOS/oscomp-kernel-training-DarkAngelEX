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
#![feature(unboxed_closures)]
#![feature(negative_impls)]
#![feature(slice_ptr_len)]
#![feature(nonzero_ops)]
#![feature(generic_arg_infer)]
#![feature(once_cell)]
#![feature(get_mut_unchecked)]
#![feature(new_uninit)]

// #![allow(dead_code)]

use riscv::register::sstatus;

extern crate alloc;
extern crate async_task;
#[macro_use]
extern crate bitflags;
extern crate riscv;
#[macro_use]
extern crate lazy_static;
extern crate easy_fs;
extern crate xmas_elf;

#[cfg(feature = "board_k210")]
#[path = "boards/k210.rs"]
mod board;
#[cfg(not(any(feature = "board_k210")))]
#[path = "boards/qemu.rs"]
mod board;

mod config;
#[macro_use]
mod console;
#[macro_use]
mod xdebug;
mod drivers;
mod executor;
mod fdt;
mod fs;
mod hart;
mod lang_items;
// mod loader;
mod local;
mod memory;
mod process;
mod signal;
mod sync;
mod syscall;
mod timer;
mod tools;
mod trap;
mod user;

///
/// This function will be called by rust_main() in hart/mod.rs
///
/// It will run on each core.
///
pub fn kmain(_hart_id: usize) -> ! {
    loop {
        executor::run_until_idle();
        // println!("sie {}", sstatus::read().sie());
        unsafe {
            sstatus::set_sie();
            riscv::asm::wfi();
            sstatus::clear_sie();
        }
    }
}
