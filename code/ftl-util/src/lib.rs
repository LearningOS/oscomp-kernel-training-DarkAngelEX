#![no_std]
#![feature(allocator_api)]
#![feature(negative_impls)]
#![feature(const_trait_impl)]
#![feature(const_fn_trait_bound)]
#![feature(ptr_const_cast)]

#[macro_use]
mod console;
#[macro_use]
mod xdebug;
pub mod async_tools;
pub mod device;
pub mod error;
pub mod list;
pub mod sync;
pub mod utc_time;

extern crate alloc;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
