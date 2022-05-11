#[cfg(not(feature = "board_hifive"))]
pub const CLOCK_FREQ: usize = 12500000;
#[cfg(feature = "board_hifive")]
pub const CLOCK_FREQ: usize = 1000000;

// pub const MMIO: &[(usize, usize)] = &[(0x10001000, 0x1000)];

pub type BlockDeviceImpl = crate::drivers::block::VirtIOBlock;
