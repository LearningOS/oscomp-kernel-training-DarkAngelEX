#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

#[no_mangle]
pub fn main() -> i32 {
    let pid = user_lib::fork();
    if pid == 0 {
        let this_pid = user_lib::getpid();
        println!("pid {} will leak", this_pid);
        user_lib::yield_();
        println!("pid {} leak", this_pid);
    }
    0
}
