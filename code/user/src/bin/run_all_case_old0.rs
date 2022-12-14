#![no_std]
#![no_main]

extern crate user_lib;

use user_lib::{exec, exit, fork, wait};

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
    let all_case = &[
        "brk\0",
        "chdir\0",
        "clone\0",
        "close\0",
        "dup2\0",
        "dup\0",
        "execve\0",
        "exit\0",
        "fork\0",
        "fstat\0",
        "getcwd\0",
        "getdents\0",
        "getpid\0",
        "getppid\0",
        "gettimeofday\0",
        "mkdir_\0",
        "mmap\0",
        "mount\0",
        "munmap\0",
        "openat\0",
        "open\0",
        "pipe\0",
        "read\0",
        "sleep\0",
        "times\0",
        "umount\0",
        "uname\0",
        "unlink\0",
        "wait\0",
        "waitpid\0",
        "write\0",
        "yield\0",
    ];
    for name in all_case {
        let _pid = match fork() {
            -1 => panic!("fork error"),
            0 => {
                exec(name, &[core::ptr::null::<u8>()]);
                unreachable!();
            }
            pid => pid,
        };
        let mut exit_code: i32 = 0;
        wait(&mut exit_code);
    }
}
