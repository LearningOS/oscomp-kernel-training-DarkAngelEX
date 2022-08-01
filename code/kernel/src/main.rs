#![no_std]
#![no_main]
#![feature(asm_const)]
#![feature(array_chunks)]
#![feature(array_try_map)]
#![feature(atomic_mut_ptr)]
#![feature(alloc_error_handler)]
#![feature(allocator_api)]
#![feature(associated_type_bounds)]
#![feature(bool_to_option)]
#![feature(bench_black_box)]
#![feature(core_intrinsics)]
#![feature(const_for)]
#![feature(const_try)]
#![feature(const_option)]
#![feature(const_convert)]
#![feature(const_mut_refs)]
#![feature(const_btree_new)]
#![feature(const_option_ext)]
#![feature(const_trait_impl)]
#![feature(const_slice_from_raw_parts)]
#![feature(custom_test_frameworks)]
#![feature(control_flow_enum)]
#![feature(default_free_fn)]
#![feature(exclusive_range_pattern)]
#![feature(generic_arg_infer)]
#![feature(get_mut_unchecked)]
#![feature(half_open_range_patterns)]
#![feature(let_chains)]
#![feature(map_first_last)]
#![feature(map_try_insert)]
#![feature(mixed_integer_ops)]
#![feature(maybe_uninit_as_bytes)]
#![feature(new_uninit)]
#![feature(never_type)]
#![feature(nonzero_ops)]
#![feature(negative_impls)]
#![feature(once_cell)]
#![feature(panic_info_message)]
#![feature(riscv_target_feature)]
#![feature(result_option_inspect)]
#![feature(str_internals)]
#![feature(split_array)]
#![feature(slice_pattern)]
#![feature(slice_ptr_len)]
#![feature(slice_ptr_get)]
#![feature(slice_from_ptr_range)]
#![feature(try_blocks)]
#![feature(try_trait_v2)]
#![feature(trait_alias)]
#![feature(trait_upcasting)]
#![feature(unboxed_closures)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]
#![allow(clippy::nonminimal_bool)]
#![allow(clippy::assertions_on_constants)]
#![allow(dead_code)]

// #![allow(dead_code)]

extern crate alloc;
extern crate async_task;
#[macro_use]
extern crate ftl_util;
extern crate vfs;
#[macro_use]
extern crate bitflags;
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
mod tools;
#[macro_use]
mod xdebug;
mod benchmark;
mod drivers;
mod executor;
mod fdt;
mod fs;
mod futex;
mod hart;
mod hifive;
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

use riscv::register::sstatus;

/// This function will be called by rust_main() in hart/mod.rs
///
/// It will run on each core.
pub fn kmain(_hart_id: usize) -> ! {
    stack_trace!(to_yellow!("running in global space"));
    let hart_local = local::hart_local();
    let always_local = local::always_local();
    assert!(always_local.sie_cur() == 0);
    assert!(always_local.sum_cur() == 0);

    unsafe {
        // 启用中断并关闭用户访问标志, 使它和always_local中的值对应
        sstatus::set_sie();
        sstatus::clear_sum();
    }
    loop {
        executor::run_until_idle();
        // println!("sie {}", sstatus::read().sie());
        hart_local.local_rcu.critical_end();
        // stack_trace!("hart idle");
        // println!("hart {} idle", hart_local.cpuid());
        unsafe {
            assert!(sstatus::read().sie());
            hart_local.enter_idle();
            riscv::asm::wfi();
            hart_local.leave_idle();
        }
        // println!("hart {} running", hart_local.cpuid());
    }
}

#[cfg(test)]
fn test_runner(tests: &[&dyn Fn()]) {
    println!("Running {} tests", tests.len());
    for test in tests {
        test();
    }
}
