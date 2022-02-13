use crate::{
    debug::trace,
    println,
    riscv::{cpu, sbi::shutdown},
};
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "Panicked at {}:{} {}",
            location.file(),
            location.line(),
            info.message().unwrap()
        );
    } else {
        println!("panicked: {}", info.message().unwrap());
    }
    if trace::OPEN_MEMORY_TRACE {
        let count = trace::current_count();
        println!("current trace count: {}", count);
    }
    trace::using_stack_size_print();
    println!("current hart {}", cpu::hart_id());
    print!("\n");
    shutdown()
}
