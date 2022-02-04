use core::{
    arch::global_asm,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::{loader, memory};

pub mod cpu;
pub mod csr;
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
        println!("[FTL OS]hart {} started", hartid);
        others_main(hartid); // -> !
    }

    // init all module there
    println!("[FTL OS]start initialization from hart {}", hartid);
    clear_bss();
    show_seg();

    crate::memory::init();

    println!("[FTL OS]hello! from hart {}", hartid);
    loader::list_apps();

    // println!("tree: {}", device_tree_paddr);

    println!("init complete! weakup the other cores.");
    AP_CAN_INIT.store(true, Ordering::Relaxed);
    crate::kmain();
}

fn others_main(hartid: usize) -> ! {
    println!("[FTL OS]hart {} init by global satp", hartid);
    unsafe { memory::set_satp_by_global() };
    println!("[FTL OS]hart {} init complete", hartid);
    crate::kmain();
}

fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}

fn show_seg() {
    extern "C" {
        fn boot_page_table_sv39();
        fn start();
        fn etext();
        fn erodata();
        fn edata();
        fn sstack();
        fn estack();
        fn sbss();
        fn ebss();
        fn end();
    }
    fn xprlntln(a: unsafe extern "C" fn(), name: &str) {
        let s = a as usize;
        println!("    {:7}: {:#x}", name, s);
    }
    println!("[FTL OS]show segment:");
    xprlntln(boot_page_table_sv39, "boot_page_table_sv39");
    xprlntln(start, "start");
    xprlntln(etext, "etext");
    xprlntln(erodata, "erodata");
    xprlntln(edata, "edata");
    xprlntln(sstack, "sstack");
    xprlntln(estack, "estack");
    xprlntln(sbss, "sbss");
    xprlntln(ebss, "ebss");
    xprlntln(end, "end");
    println!("    cur sp : {:#x}", csr::get_sp());
}
