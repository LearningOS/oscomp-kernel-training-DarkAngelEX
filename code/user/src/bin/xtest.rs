#![no_std]
#![no_main]

extern crate alloc;
extern crate user_lib;

use user_lib::*;

#[no_mangle]
fn main() -> i32 {
    match fork() {
        0 => {
            test_fn();
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

fn test_fn() {
    let n = fork();
    assert!(n >= 0);
    if n == 0 {
        let name = "12345";
        exec(&"1234", &[name.as_ptr(), core::ptr::null()]);
        println!("exec fail!");
        // exit(-123456);
        unreachable!();
    }
    let mut code = 0;
    wait(&mut code);
    if code == -123456 {
        return;
    }
}
