use core::{
    arch::global_asm,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

mod cpu;

global_asm!(include_str!("./boot/entry64.asm"));

static AP_CAN_INIT: AtomicBool = AtomicBool::new(false);
static FIRST_HART: AtomicBool = AtomicBool::new(false);
static DEVICE_TREE_PADDR: AtomicUsize = AtomicUsize::new(0);

const BOOT_HART_ID: usize = 0;


#[no_mangle]
pub extern "C" fn rust_main(hartid: usize, device_tree_paddr: usize) -> ! {
    unsafe { cpu::set_cpu_id(hartid) };
    if FIRST_HART
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        DEVICE_TREE_PADDR.store(device_tree_paddr, Ordering::SeqCst);
    }

    if hartid != BOOT_HART_ID {
        while !AP_CAN_INIT.load(Ordering::Relaxed) {}
        crate::println!("hart {} started", hartid);
        others_main(hartid); // -> !
    }

    // init all module there
    crate::println!("hello FTLOS! from hart {}", hartid);

    AP_CAN_INIT.store(true, Ordering::Relaxed);
    crate::kmain();
}

fn others_main(hartid: usize) -> ! {
    unsafe {
        // trapframe::init();
    }
    // memory::init_other();
    // timer::init();
    // info!("Hello RISCV! in hart {}", hartid);
    crate::kmain();
}

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}
