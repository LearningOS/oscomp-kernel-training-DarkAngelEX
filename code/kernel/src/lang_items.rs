use crate::{
    hart::{cpu, sbi},
    local,
    xdebug::{stack_trace, trace},
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
    if stack_trace::STACK_TRACE {
        match local::hart_local().try_task() {
            None => {
                println!("stack trace: empty");
            }
            Some(task) => {
                println!("stack_trace:");
                task.stack_trace.print_all_stack()
            }
        }
    }
    println!("shutdown!!");
    // loop {}
    sbi::shutdown()
}
