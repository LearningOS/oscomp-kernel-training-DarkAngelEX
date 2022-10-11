#![no_std]
#![no_main]

extern crate alloc;
extern crate user_lib;

use alloc::{string::ToString, vec::Vec};
use user_lib::*;

const PRINT_LINE: bool = false;

#[no_mangle]
fn main() -> i32 {
    match fork() {
        0 => {
            let name = "/busybox\0";
            exec(&name, &[name.as_ptr(), "sh\0".as_ptr(), core::ptr::null()]);
            exit(0);
        }
        1.. => {
            let mut exit_code: i32 = 0;
            wait(&mut exit_code);
        }
        _ => panic!("initproc fork error"),
    }
    exit(0)
}
