use core::{
    arch::{asm, global_asm},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use crate::{
    benchmark, console, drivers, executor, fs, local, memory, process, timer,
    tools::{self, container},
    trap,
    user::{self, AutoSie},
    xdebug::{self, CLOSE_TIME_INTERRUPT},
};

pub mod cpu;
pub mod csr;
pub mod floating;
pub mod interrupt;
pub mod sbi;
pub mod sfence;

global_asm!(include_str!("./boot/entry64.asm"));

static INIT_START: AtomicBool = AtomicBool::new(false);
static INIT_HART: AtomicUsize = AtomicUsize::new(usize::MAX);
static AP_CAN_INIT: AtomicBool = AtomicBool::new(false);
static AP_INIT_WAIT: AtomicUsize = AtomicUsize::new(0);
static AP_CAN_RUN: AtomicBool = AtomicBool::new(false);
#[link_section = "data"]
static FIRST_HART: AtomicBool = AtomicBool::new(false);
#[link_section = "data"]
static DEVICE_TREE_PADDR: AtomicUsize = AtomicUsize::new(0);

const BOOT_HART_ID: usize = 0;

// pub fn device_tree_ptr() -> PhyAddr {
//     DEVICE_TREE_PADDR.load(Ordering::Relaxed).into()
// }

// fn show_device() {
//     println!("[FTL OS]show device");
//     let ptr = device_tree_ptr();
//     let ptr = ptr.into_usize() as *mut FdtHeader;
//     let x = unsafe { &*ptr };
//     println!("fdt ptr: {:#x}", ptr as usize);
//     println!("{:?}", x);
//     panic!();
// }

/// FTL OS logo
///
/// generate from http://patorjk.com/software/taag/ with font `Speed`
pub fn ftl_logo() -> &'static str {
    concat!(
        concat!(r#"______________________    _______________"#, '\n'),
        concat!(r#"___  ____/__  __/__  /   ___  __ \/  ___/"#, '\n'),
        concat!(r#"__  /_   __  /  __  /   ___  / / /____ \ "#, '\n'),
        concat!(r#"_  __/  __  /  __  /___   / /_/ /____/ / "#, '\n'),
        concat!(r#"/_/      /_/    /_____/   \____//_____/  "#, '\n'),
        concat!(r#"  - - - - Faster  Than  Light - - - -    "#, '\n'),
    )
}

macro_rules! smp_v {
    ($a: ident => $v: literal) => {
        while $a.load(Ordering::Acquire) != $v {
            core::hint::spin_loop();
        }
    };
    ($v: literal => $a: ident) => {
        $a.store($v, Ordering::Release);
    };
}

#[no_mangle]
pub extern "C" fn rust_main(hartid: usize, device_tree_paddr: usize) -> ! {
    unsafe {
        cpu::set_gp(); // 愚蠢的rust链接器不支持linker relax, 未使用
        cpu::set_hart_local(hartid);
    };
    if FIRST_HART
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        clear_bss();
        xdebug::init();
        console::init();
        println!("[FTL OS]version 0.1.0");
        println!("{}", ftl_logo());
        INIT_HART.store(hartid, Ordering::Release);
        // #[cfg(feature = "board_hifive")]
        {
            for i in (0..=4).filter(|&i| i != hartid) {
                let status = sbi::sbi_hart_get_status(i);
                println!("hart {} status {}", i, status);
                sbi::sbi_hart_start(i, 0x80200000, 0);
            }
        }
        INIT_START.store(true, Ordering::Release);
    } else {
        while !INIT_START.load(Ordering::Acquire) {}
    }
    local::init();
    println!(
        "[FTL OS]hart {} device tree: {:#x}",
        hartid, device_tree_paddr
    );
    if device_tree_paddr != 0 {
        DEVICE_TREE_PADDR.store(device_tree_paddr, Ordering::Release);
    }
    unsafe { cpu::init(hartid) };
    local::set_stack();
    if hartid != INIT_HART.load(Ordering::Acquire) {
        others_main(hartid); // -> !!!!!!!!!!!!!!! main !!!!!!!!!!!!!!!
    }
    // init all module there
    println!("[FTL OS]start initialization from hart {}", hartid);
    show_seg();
    for _i in 0..100000 {
        // waitting cpu::increase
        AP_CAN_INIT.load(Ordering::Acquire);
    }
    println!("[FTL OS]CPU count: {}", cpu::count());
    println!(
        "[FTL OS]device tree physical address: {:#x}",
        DEVICE_TREE_PADDR.load(Ordering::Acquire)
    );
    // assert!(DEVICE_TREE_PADDR.load(Ordering::Relaxed) != 0);
    // show_device();
    trap::init();
    memory::init();
    container::test();
    timer::init();
    executor::init();
    floating::init();
    benchmark::run_all();
    #[cfg(feature = "board_hifive")]
    crate::hifive::prci::overclock_1500mhz();
    benchmark::run_all();
    drivers::init();
    #[cfg(test)]
    crate::test_main();
    executor::kernel_spawn(async move {
        println!("[FTL OS]running async init");
        drivers::test().await;
        fs::init().await;
        fs::list_apps().await;
        process::init().await;
        user::test().await;
        println!("[FTL OS]hello! from hart {}", hartid);
        sfence::fence_i();
        println!("init complete! weakup the other cores.");
        AP_INIT_WAIT.store(cpu::count() - 1, Ordering::Release);
        smp_v!(true => AP_CAN_INIT);
        {
            let _sie = AutoSie::new();
            tools::multi_thread_test(hartid);
        }
        smp_v!(AP_INIT_WAIT => 0);
        smp_v!(true => AP_CAN_RUN);
        if !CLOSE_TIME_INTERRUPT {
            trap::enable_timer_interrupt();
            timer::set_next_trigger();
        }
    });
    crate::kmain(hartid);
}

fn others_main(hartid: usize) -> ! {
    while !AP_CAN_INIT.load(Ordering::Acquire) {}
    println!("[FTL OS]hart {} started", hartid);
    println!("[FTL OS]hart {} init by global satp", hartid);
    memory::set_satp_by_global();
    sfence::sfence_vma_all_global();
    sfence::fence_i();
    unsafe { trap::set_kernel_default_trap() };
    floating::other_init();
    // local::init();
    tools::multi_thread_test(hartid);
    if !CLOSE_TIME_INTERRUPT {
        trap::enable_timer_interrupt();
        timer::set_next_trigger();
    }
    println!("[FTL OS]hart {} init complete", hartid);
    AP_INIT_WAIT.fetch_sub(1, Ordering::Release);
    smp_v!(AP_CAN_RUN => true);
    crate::kmain(hartid);
}

/// clear bss to set some variable into zero.
fn clear_bss() {
    extern "C" {
        fn sbss();
        fn ebss();
    }
    unsafe {
        let sbss = sbss as *mut usize;
        let ebss = ebss as *mut usize;
        core::slice::from_mut_ptr_range(sbss..ebss).fill(0);
    }
    // (sbss as usize..ebss as usize).for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
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
    let kernel_size = end as usize - start as usize;
    let (m, k, b) = tools::size_to_mkb(kernel_size);
    println!("kernel static size: {}MB {}KB {}Bytes", m, k, b);
}

pub fn current_sp() -> usize {
    let ret: usize;
    unsafe {
        asm!("mv {}, sp", out(reg)ret);
    }
    ret
}
