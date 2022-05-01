#![no_std]
#![feature(allocator_api)]
#![feature(negative_impls)]
#![feature(const_trait_impl)]
#![feature(const_fn_trait_bound)]

pub mod error;
pub mod sync;

extern crate alloc;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}
