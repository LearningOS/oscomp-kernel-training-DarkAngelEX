#![no_std]
#![no_main]
#![feature(panic_info_message)]

mod riscv;
mod console;
mod lang_items;
mod sbi;


pub fn kmain() -> ! {
    loop {}
}
