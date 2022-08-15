#![no_std]
#![feature(allocator_api)]
#![feature(atomic_mut_ptr)]
#![feature(assert_matches)]
#![feature(box_into_inner)]
#![feature(core_intrinsics)]
#![feature(const_trait_impl)]
#![feature(if_let_guard)]
#![feature(int_roundings)]
#![feature(negative_impls)]
#![feature(ptr_const_cast)]
#![feature(sync_unsafe_cell)]
#![feature(untagged_unions)]
#![feature(unboxed_closures)]

use crate::rcu::RcuDrop;
use xdebug::stack::XInfo;

#[macro_use]
extern crate bitflags;

#[macro_use]
pub mod xmarco;
#[macro_use]
pub mod console;
#[macro_use]
pub mod xdebug;
#[macro_use]
pub mod list;
pub mod async_tools;
pub mod container;
pub mod device;
pub mod error;
pub mod faster;
pub mod fs;
pub mod local;
pub mod rcu;
pub mod sync;
pub mod time;

extern crate alloc;

pub const MAX_CPU: usize = 32;

pub fn debug_init(push_fn: fn(XInfo, &'static str, u32), pop_fn: fn(), current_sie: fn() -> bool) {
    xdebug::stack::init(push_fn, pop_fn);
    xdebug::sie_init(current_sie);
}

pub fn console_init(write_fn: fn(core::fmt::Arguments)) {
    console::init(write_fn)
}

pub fn rcu_init(rcu_drop_fn: fn(RcuDrop)) {
    rcu::init(rcu_drop_fn)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
