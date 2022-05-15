use crate::config::DIRECT_MAP_OFFSET;

use super::registers::*;

const PRCI_ADDR: usize = 0x1000_0000;
const PRCI_ADDR_REF: usize = PRCI_ADDR + DIRECT_MAP_OFFSET;

pub fn prci_block() -> &'static mut RegisterBlock {
    unsafe { &mut *(PRCI_ADDR_REF as *mut RegisterBlock) }
}

#[doc = "For more information, see: https://sifive.cdn.prismic.io/sifive/d3ed5cd0-6e74-46b2-a12d-72b06706513e_fu540-c000-manual-v1p4.pdf"]
#[repr(C)]
pub struct RegisterBlock {
    #[doc = "0x00: Crystal Oscillator Configuration and Status"]
    pub hfxosccfg: UNUSEDNOW,
    #[doc = "0x04: PLL Configuration and Status"]
    pub core_pllcfg: CorePllCfg,
    #[doc = "0x08: PLL Final Divide Configuration"]
    pub core_plloutdiv: UNUSEDNOW,
    #[doc = "0x0C: PLL Configuration and Status"]
    pub ddr_pllcfg: UNUSEDNOW,
    #[doc = "0x10: PLL Final Divide Configuration"]
    pub ddr_plloutdiv: UNUSEDNOW,
    #[doc = "0x14 - 0x18"]
    pub __unused_0: [RESERVED; 2],
    #[doc = "0x1C: PLL Configuration and Status"]
    pub gemgxl_pllcfg: UNUSEDNOW,
    #[doc = "0x20: PLL Final Divide Configuration"]
    pub gemgxl_plloutdiv: UNUSEDNOW,
    #[doc = "0x24: Select core clock source. 0: coreclkpll 1: external hfclk"]
    pub core_clk_sel_reg: CoreClkSelReg,
    #[doc = "0x28: Software controlled resets (active low)"]
    pub devices_reset_n: UNUSEDNOW,
    #[doc = "0x2C: Current selection of each clock mux"]
    pub clk_mux_status: UNUSEDNOW,
    #[doc = "0x30 - 0x34"]
    pub __unused_1: [RESERVED; 2],
    #[doc = "0x38: PLL Configuration and Status"]
    pub dvfs_core_pllcfg: UNUSEDNOW,
    #[doc = "0x3C: PLL Final Divide Configuration"]
    pub dvfs_core_plloutdiv: UNUSEDNOW,
    #[doc = "0x40: Select which PLL output to use for core clock. 0: corepll 1: dvfscorepll"]
    pub corepllsel: UNUSEDNOW,
    #[doc = "0x44 - 0x4C"]
    pub __unused_2: [RESERVED; 3],
    #[doc = "0x50: PLL Configuration and Status"]
    pub hfpclk_pllcfg: UNUSEDNOW,
    #[doc = "0x54: PLL Final Divide Configuration"]
    pub hfpclk_plloutdiv: UNUSEDNOW,
    #[doc = "0x58: Select source for Periphery Clock (pclk). 0: hfpclkpll 1: external hfclk"]
    pub hfpclkpllsel: UNUSEDNOW,
    #[doc = "0x5C: HFPCLK PLL divider value"]
    pub hfpclk_div_reg: UNUSEDNOW,
    #[doc = "0x60 - 0xDC"]
    pub __unused_3: [RESERVED; 32],
    #[doc = "0xE0: Indicates presence of each PLL"]
    pub prci_plls: UNUSEDNOW,
}
