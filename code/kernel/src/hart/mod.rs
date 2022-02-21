use core::{
    arch::{asm, global_asm},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::{
    executor, loader,
    memory::{self, address::PhyAddr},
    process, timer, trap,
    xdebug::CLOSE_TIME_INTERRUPT, fdt::FdtHeader,
};

pub mod cpu;
pub mod csr;
pub mod interrupt;
pub mod sbi;
pub mod sfence;

global_asm!(include_str!("./boot/entry64.asm"));

static INIT_START: AtomicBool = AtomicBool::new(false);
static AP_CAN_INIT: AtomicBool = AtomicBool::new(false);
#[link_section = "data"]
static FIRST_HART: AtomicBool = AtomicBool::new(false);
#[link_section = "data"]
static DEVICE_TREE_PADDR: AtomicUsize = AtomicUsize::new(0);

const BOOT_HART_ID: usize = 0;

#[allow(dead_code)]
pub fn device_tree_ptr() -> PhyAddr {
    DEVICE_TREE_PADDR.load(Ordering::Relaxed).into()
}

#[allow(dead_code)]
fn show_device() {
    println!("[FTL OS]show device");
    let ptr = device_tree_ptr();
    let ptr = ptr.into_usize() as *mut FdtHeader;
    let x = unsafe { &*ptr };
    println!("fdt ptr: {:#x}", ptr as usize);
    println!("{:?}", x);
    panic!();
}

#[no_mangle]
pub extern "C" fn rust_main(hartid: usize, device_tree_paddr: usize) -> ! {
    unsafe { cpu::set_cpu_id(hartid) };
    if FIRST_HART
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        clear_bss();
        INIT_START.store(true, Ordering::Release);
        println!("[FTL OS]clear bss using hart {}", hartid);
    } else {
        while !INIT_START.load(Ordering::Acquire) {}
    }
    println!(
        "[FTL OS]hart {} device tree: {:#x}",
        hartid, device_tree_paddr
    );
    if device_tree_paddr != 0 {
        // DEVICE_TREE_PADDR.compare_exchange(current, new, success, failure);
        DEVICE_TREE_PADDR.store(device_tree_paddr, Ordering::Release);
    }

    unsafe { cpu::increase_cpu() };
    if hartid != BOOT_HART_ID {
        while !AP_CAN_INIT.load(Ordering::Acquire) {}
        println!("[FTL OS]hart {} started", hartid);
        others_main(hartid); // -> !!!!!!!!!!!!!!! main !!!!!!!!!!!!!!!
    }
    // init all module there
    println!("[FTL OS]start initialization from hart {}", hartid);
    show_seg();
    println!("[FTL OS]CPU count: {}", cpu::count());
    println!(
        "[FTL OS]device tree physical address: {:#x}",
        DEVICE_TREE_PADDR.load(Ordering::Acquire)
    );
    // assert!(DEVICE_TREE_PADDR.load(Ordering::Relaxed) != 0);
    // show_device();

    memory::init();
    timer::init();
    executor::init();
    trap::init();
    process::init();
    if !CLOSE_TIME_INTERRUPT {
        trap::enable_timer_interrupt();
        timer::set_next_trigger();
    }
    println!("[FTL OS]hello! from hart {}", hartid);
    loader::list_apps();

    sfence::fence_i();
    println!("init complete! weakup the other cores.");
    AP_CAN_INIT.store(true, Ordering::Release);
    crate::kmain(hartid);
}

fn others_main(hartid: usize) -> ! {
    println!("[FTL OS]hart {} init by global satp", hartid);
    memory::set_satp_by_global();
    sfence::sfence_vma_all_global();
    sfence::fence_i();
    unsafe { trap::set_kernel_trap_entry() };
    if !CLOSE_TIME_INTERRUPT {
        trap::enable_timer_interrupt();
        timer::set_next_trigger();
    }
    println!("[FTL OS]hart {} init complete", hartid);
    crate::kmain(hartid);
}

/// clear bss to set some variable into zero.
fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    (sbss as usize..ebss as usize).for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}

fn show_seg() {
    extern "C" {
        // fn boot_page_table_sv39();
        fn start();
        fn etext();
        fn srodata();
        fn erodata();
        fn sdata();
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
    // xprlntln(boot_page_table_sv39, "boot_page_table_sv39");
    xprlntln(start, "start");
    xprlntln(etext, "etext");
    xprlntln(srodata, "srodata");
    xprlntln(erodata, "erodata");
    xprlntln(sdata, "sdata");
    xprlntln(edata, "edata");
    xprlntln(sstack, "sstack");
    println!("    cur sp : {:#x}", csr::get_sp());
    xprlntln(estack, "estack");
    xprlntln(sbss, "sbss");
    xprlntln(ebss, "ebss");
    xprlntln(end, "end");
}

pub fn current_sp() -> usize {
    let ret: usize;
    unsafe {
        asm!("mv {}, sp", out(reg)ret);
    }
    ret
}
