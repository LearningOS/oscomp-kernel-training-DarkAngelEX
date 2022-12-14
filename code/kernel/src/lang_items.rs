use crate::{
    console,
    hart::{cpu, sbi},
    xdebug::trace,
};
use core::panic::PanicInfo;

#[panic_handler]
#[inline(never)]
fn panic(info: &PanicInfo) -> ! {
    console::disable_getchar();
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
    #[cfg(feature = "stack_trace")]
    {
        use crate::local;
        println!("stack_trace hart: {}", cpu::hart_id());
        local::always_local().stack_trace.print_all_stack();
        for i in cpu::hart_range() {
            if i == cpu::hart_id() {
                continue;
            }
            println!("stack_trace hart: {}", i);
            unsafe { local::get_local_by_id(i) }
                .always_ref()
                .stack_trace
                .print_all_stack();
        }
    }
    println!("!TEST FINISH!");
    println!("shutdown!!");
    // loop {}
    sbi::shutdown()
}
