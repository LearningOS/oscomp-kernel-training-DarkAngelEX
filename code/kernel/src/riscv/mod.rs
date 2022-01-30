use core::{
    arch::global_asm,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub mod cpu;
pub mod interrupt;
pub mod sbi;

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
        println!("hart {} started", hartid);
        others_main(hartid); // -> !
    }

    // init all module there
    println!("init FTLOS from hart {}", hartid);
    clear_bss();
    crate::mm::init();
    println!("hello FTLOS! from hart {}", hartid);
    extern "C" {
        fn boot_page_table_sv39();
        fn start();
        fn etext();
        fn erodata();
        fn edata();
        fn sbss();
        fn ebss();
        fn end();
    }
    fn xprlntln(a: unsafe extern "C" fn(), name: &str) {
        let s = a as usize;
        println!("{:7}: {:#x}", name, s);
    }
    println!("tree: {}", device_tree_paddr);

    xprlntln(boot_page_table_sv39, "boot_page_table_sv39");
    xprlntln(start, "start");
    xprlntln(etext, "etext");
    xprlntln(erodata, "erodata");
    xprlntln(edata, "edata");
    xprlntln(sbss, "sbss");
    xprlntln(ebss, "ebss");
    xprlntln(end, "end");

    println!("init complete! weakup the other cores.");
    AP_CAN_INIT.store(true, Ordering::Relaxed);
    crate::kmain();
}

fn others_main(hartid: usize) -> ! {
    crate::kmain();
}

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}

fn xget(a: usize) -> &'static mut usize {
    let a = a as *mut usize;
    unsafe { &mut *a }
}
