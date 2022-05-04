#![no_std]
#![feature(allocator_api)]
#![feature(negative_impls)]
#![feature(const_trait_impl)]
#![feature(const_fn_trait_bound)]
#![feature(ptr_const_cast)]

use xdebug::stack::XInfo;

#[macro_use]
pub mod console;
#[macro_use]
pub mod xdebug;
pub mod async_tools;
pub mod device;
pub mod error;
pub mod list;
pub mod sync;
pub mod utc_time;

extern crate alloc;

pub fn debug_init(push_fn: fn(XInfo, &'static str, u32), pop_fn: fn(), current_sie: fn() -> bool) {
    xdebug::stack::init(push_fn, pop_fn);
    xdebug::sie_init(current_sie);
}

pub fn console_init(write_fn: fn(core::fmt::Arguments)) {
    console::init(write_fn)
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
