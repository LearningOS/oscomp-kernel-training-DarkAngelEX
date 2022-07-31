use core::{fmt::Display, mem::MaybeUninit};

use alloc::boxed::Box;
use ftl_util::device::BlockDevice;

use crate::tools;

/// 处于FAT32保留区
///
/// 通常位于逻辑扇区1
pub struct RawFsInfo {
    pub signature_head: u32,  // must be 0x41615252
    pub signature_1: u32,     // must be 0x61417272
    pub cluster_free: u32,    // 空簇数
    pub cluster_next: u32,    // 下一个可用簇号 如果为 0xFFFFFFFF 将从2开始搜索
    pub reversed_1: [u8; 12], // 保留
    pub signature_trail: u32, // must be 0xAA550000
}

impl RawFsInfo {
    pub const fn zeroed() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }

    pub async fn load(&mut self, info_cluster_id: usize, device: &dyn BlockDevice) {
        let mut buf: Box<[u8]> =
            unsafe { Box::new_uninit_slice(device.sector_bytes()).assume_init() };
        let sector = device.sector_bpb() + info_cluster_id;
        device.read_block(sector, &mut buf).await.unwrap();
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
        load!(self.reversed_1);
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
    pub fn raw_store(cluster_free: u32, cluster_next: u32, dst: &mut [u8]) {
        let mut offset: usize = 0x0;
        assert!(dst.len() >= 512);
        macro_rules! store {
            ($v: expr) => {
                tools::store_fn(&$v, dst, &mut offset);
            };
        }
        offset += 4;
        offset += 480;
        offset += 4;
        store!(cluster_free);
        store!(cluster_next);
        offset += 12;
        offset += 4;
        debug_assert_eq!(offset, 512);
    }
}

impl Display for RawFsInfo {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "signature_head:  {:#X}", self.signature_head)?;
        writeln!(f, "signature_1:     {:#X}", self.signature_1)?;
        writeln!(f, "cluster_free:    {}", self.cluster_free)?;
        writeln!(f, "cluster_next:    {}", self.cluster_next)?;
        writeln!(f, "reversed_1:      {:?}", self.reversed_1)?;
        writeln!(f, "signature_trail: {:#X}", self.signature_trail)?;
        Ok(())
    }
}
