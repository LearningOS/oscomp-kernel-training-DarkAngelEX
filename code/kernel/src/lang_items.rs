use crate::{debug::trace, println, riscv::sbi::shutdown};
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
        let count =trace::current_count();
        println!("current trace count: {}", count);
    }
    trace::using_stack_size_print();
    print!("\n");
    shutdown()
}
