#![no_std]
#![no_main]
#![feature(panic_info_message)]
#![feature(const_slice_from_raw_parts)]
#![feature(alloc_error_handler)]
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
#![feature(const_trait_impl)]
#![feature(const_try)]
#![feature(const_mut_refs)]
#![feature(const_option)]
#![feature(const_convert)]
#![feature(const_for)]
#![feature(associated_type_bounds)]
#![feature(let_chains)]
#![feature(result_option_inspect)]
#![feature(half_open_range_patterns)]
#![feature(exclusive_range_pattern)]
#![feature(riscv_target_feature)]
#![feature(slice_ptr_get)]
#![feature(maybe_uninit_as_bytes)]
#![feature(custom_test_frameworks)]
#![feature(allocator_api)]
#![feature(str_internals)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(dead_code)]

// #![allow(dead_code)]

extern crate alloc;
extern crate async_task;
#[macro_use]
extern crate ftl_util;
#[macro_use]
extern crate bitflags;
extern crate easy_fs;
extern crate fat32;
extern crate riscv;
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
#[macro_use]
mod tools;
mod benchmark;
mod drivers;
mod executor;
mod fdt;
mod fs;
mod hart;
mod lang_items;
mod local;
mod memory;
mod process;
mod signal;
mod sync;
mod syscall;
mod timer;
mod trap;
mod user;
pub mod hifive;

use riscv::register::sstatus;

///
/// This function will be called by rust_main() in hart/mod.rs
///
/// It will run on each core.
///
pub fn kmain(_hart_id: usize) -> ! {
    stack_trace!(to_yellow!("running in global space"));
    let local = local::always_local();
    assert!(local.sie_cur() == 0);
    assert!(local.sum_cur() == 0);

    unsafe {
        sstatus::set_sie();
        sstatus::clear_sum();
    }
    loop {
        executor::run_until_idle();
        // println!("sie {}", sstatus::read().sie());
        unsafe {
            assert!(sstatus::read().sie());
            riscv::asm::wfi();
        }
    }
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
}
