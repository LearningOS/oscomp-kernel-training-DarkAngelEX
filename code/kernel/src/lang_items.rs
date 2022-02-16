use crate::{
    xdebug::{stack_trace, trace},
    println,
    riscv::{cpu, sbi::shutdown},
    scheduler,
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
    if stack_trace::STACK_TRACE {
        let ptr = scheduler::get_current_stack_trace();
        if ptr == core::ptr::null_mut() {
            println!("stack trace: empty");
        } else {
            unsafe { (*ptr).print_all_stack() }
        }
    }
    println!("loop forever!!");
    loop {}
    // shutdown()
}
