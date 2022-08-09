#![no_std]
#![no_main]
#![allow(dead_code)]

extern crate alloc;
extern crate user_lib;

use alloc::{string::ToString, vec::Vec};
use user_lib::*;

const PRINT_LINE: bool = false;

#[no_mangle]
fn main() -> i32 {
    match fork() {
        0 => {
            run_all_case();
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

fn run_all_case() {
    // run_sh("/busybox_testcode.sh\0");
    // run_sh("/lua_testcode.sh\0");
    run_sh("/lmbench_testcode.sh\0");
}

fn run_sh(path: &str) {
    println!("running {}", path);
    let n = fork();
    if n < 0 {
        println!("fork fail! err: {}", n);
        exit(-1);
    }
    if n == 0 {
        let name = path;
        exec(&name, &[name.as_ptr(), core::ptr::null()]);
        println!("exec fail!");
        exit(-2);
    }
    let mut code = 0;
    wait(&mut code);
}
