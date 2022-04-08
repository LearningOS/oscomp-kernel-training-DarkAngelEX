use core::{fmt::Display, mem::MaybeUninit};

use alloc::boxed::Box;

use crate::{tools, BlockDevice};

/// 处于FAT32保留区
///
/// 通常位于逻辑扇区1
pub struct RawFsInfo {
    pub signature_head: u32, // must be 0x41615252
    // pub reversed_0: [u8; 480], 都为0
    pub signature_1: u32,     // must be 0x61417272
    pub cluster_free: u32,    // 空簇数
    pub cluster_next: u32,    // 下一个可用簇号 如果为 0xFFFFFFFF 将从2开始搜索
    pub reversed_0: [u8; 12], // 保留
    pub signature_trail: u32, // must be 0xAA550000
}

impl RawFsInfo {
    pub fn zeroed() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    pub async fn load(&mut self, device: &impl BlockDevice) {
        let mut buf: Box<[u8]> =
            unsafe { Box::new_uninit_slice(device.sector_bytes()).assume_init() };
        device
            .read_block(device.sector_bpb() + 1, &mut buf)
            .await
            .unwrap();
        self.raw_load(&buf);
    }
    pub fn raw_load(&mut self, src: &[u8]) {
        let mut offset: usize = 0x0;
        macro_rules! load {
            ($v: expr) => {
                tools::load_fn(&mut $v, src, &mut offset);
            };
        }
        assert!(src.len() >= 512);
        load!(self.signature_head);
        offset += 480;
        load!(self.signature_1);
        load!(self.cluster_free);
        load!(self.cluster_next);
        load!(self.reversed_0);
        load!(self.signature_trail);
        debug_assert_eq!(offset, 512);
        self.signature_check().unwrap();
    }
    pub fn signature_check(&self) -> Result<(), ()> {
        assert_eq!(self.signature_head, 0x41615252);
        assert_eq!(self.signature_1, 0x61417272);
        assert_eq!(self.signature_trail, 0xAA550000);
        Ok(())
    }
}

impl Display for RawFsInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "signature_head:  {:#X}\n", self.signature_head)?;
        write!(f, "signature_1:     {:#X}\n", self.signature_1)?;
        write!(f, "cluster_free:    {}\n", self.cluster_free)?;
        write!(f, "cluster_next:    {}\n", self.cluster_next)?;
        write!(f, "reversed_0:      {:?}\n", self.reversed_0)?;
        write!(f, "signature_trail: {:#X}\n", self.signature_trail)?;
        Ok(())
    }
}