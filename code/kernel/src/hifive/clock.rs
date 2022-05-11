use crate::config::DIRECT_MAP_OFFSET;

pub const PRCI_ADDR: usize = 0x1000_0000 + DIRECT_MAP_OFFSET;

pub const HF_CLK: usize = 26_000_000;
pub const HFPCLKPLL: usize = 104_000_000;

/// 对HF_CLK = 26MHz 时钟倍频 输出范围 20MHz ~ 2.4GHz
///
/// 1.5 GHz: (0, 57, 1) 1508MHz
///
/// 1 GHz: (0, 76, 2) 1001MHz
///
/// 750 MHz: (0, 57, 2)
///
/// 500 MHz: (0, 76, 3)
///
/// 250 MHz: (0, 76, 4)
///
/// 125 MHz: (0, 76, 5)
///
/// 38.1875 MHz: (0, 46, 6)
pub const fn hfclk_pll_freq(pllr: usize, pllf: usize, pllq: usize) -> usize {
    debug_assert!(pllr < 1 << 6); // 64
    debug_assert!(pllf < 1 << 9); // 512
    debug_assert!(pllq < 1 << 3); // 8
    debug_assert!(pllq <= 6);
    let post_div = HF_CLK / (pllr + 1);
    debug_assert!(post_div >= 7000_000);
    let pll_veo = post_div * 2 * (pllf + 1);
    debug_assert!(2400_000_000 <= pll_veo && pll_veo <= 4800_000_000);
    debug_assert!(pllq <= 6);
    let output = pll_veo >> pllq;
    debug_assert!(20_000_000 <= output && output <= 2400_000_000);
    output
}
