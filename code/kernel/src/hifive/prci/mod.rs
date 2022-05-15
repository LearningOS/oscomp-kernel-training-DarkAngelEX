pub mod layout;
pub mod registers;

/// 从默认的1200MHz超频至1500MHz
pub fn overclock_1500mhz() {
    let prci = layout::prci_block();
    prci.core_clk_sel_reg.using_hfclk();
    prci.core_pllcfg.set_1500mhz();
    prci.core_pllcfg.wait_lock();
    prci.core_clk_sel_reg.using_coreclk();
}
