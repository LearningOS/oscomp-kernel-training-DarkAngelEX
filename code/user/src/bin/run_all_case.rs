#![no_std]
#![no_main]

extern crate alloc;
extern crate user_lib;

use alloc::{string::ToString, vec::Vec};
use user_lib::{exec, exit, fork, open, println, read, wait, write, OpenFlags};

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
            let pid = wait(&mut exit_code);
        }
        _ => panic!("initproc fork error"),
    }
    exit(0)
}

fn run_all_case() {
    run_sh("./run-static.sh\0");
    run_sh("./run-dynamic.sh\0");
}

fn run_sh(path: &str) {
    let v = open(path, OpenFlags::RDONLY);
    assert!(v > 0);
    let mut buf = Vec::new();
    buf.resize(10000, 0);
    let n = read(v as usize, &mut buf[..]) as usize;
    assert!(n != 0 && n < buf.len());
    for line in buf[..n].split(|&c| c == b'\n') {
        let line = alloc::str::from_utf8(line).unwrap();
        if PRINT_LINE {
            println!("line: {}", line);
        }
        let mut it = line.as_bytes().split(|&c| c == b' ');
        match (it.next(), it.next(), it.next(), it.next()) {
            (Some(n), Some(a), Some(b), Some(c)) => {
                let n = alloc::str::from_utf8(n).unwrap();
                let a = alloc::str::from_utf8(a).unwrap();
                let b = alloc::str::from_utf8(b).unwrap();
                let c = alloc::str::from_utf8(c).unwrap();
                run_item(n, a, b, c);
            }
            _ => return,
        }
    }
}

fn run_item(name: &str, a: &str, b: &str, c: &str) {
    if PRINT_LINE {
        println!("<{}> <{}> <{}> <{}>", name, a, b, c);
    }
    // if c == "tls_get_new_dtv" {
    //     println!("skip tls_get_new_dtv !!");
    //     return;
    // }
    let n = fork();
    assert!(n >= 0);
    if n == 0 {
        let mut name = name.to_string();
        let mut a = a.to_string();
        let mut b = b.to_string();
        let mut c = c.to_string();
        name.push_str("\0");
        a.push_str("\0");
        b.push_str("\0");
        c.push_str("\0");
        exec(
            &name,
            &[
                name.as_ptr(),
                a.as_ptr(),
                b.as_ptr(),
                c.as_ptr(),
                core::ptr::null(),
            ],
        );
        println!("exec fail!");
        exit(-123456);
    }
    let mut code = 0;
    wait(&mut code);
    if code == -123456 {
        return;
    }
}
