use alloc::{collections::BTreeSet, vec::Vec};

use crate::{
    layout::bpb::RawBPB,
    tools::{self, CID, SID},
    BlockDevice,
};

/// 放置于内存的FAT表
///
/// 一个扇区放置128个CID
pub struct FatList {
    list: Vec<CID>,       // 整个FAT表
    free: Vec<CID>,       // 空闲FAT表分配器
    dirty: BTreeSet<SID>, // 已修改的FAT扇区
    start: SID,           // 此FAT开始的扇区
    size: usize,          // 簇数 list中超过size的将被忽略
}

impl FatList {
    pub const fn empty() -> Self {
        Self {
            list: Vec::new(),
            free: Vec::new(),
            dirty: BTreeSet::new(),
            start: SID(0),
            size: 0,
        }
    }
    /// 加载第 n 个副本
    pub async fn load(&mut self, bpb: &RawBPB, n: usize, device: &dyn BlockDevice) {
        assert!(n < bpb.fat_num as usize);
        self.start = SID(bpb.fat_sector_start.0 as u32 + bpb.sector_per_fat * n as u32);
        self.size = bpb.data_cluster_num;
        let cid_per_sector = bpb.sector_bytes as usize / core::mem::size_of::<CID>();
        // list的长度 对齐到簇
        let len = bpb.sector_per_fat as usize * cid_per_sector;
        self.list.resize(len, CID(0));
        let buf = tools::to_bytes_slice_mut(&mut self.list);
        device.read_block(self.start.0 as usize, buf).await.unwrap();
        for &cid in self.list.iter().take(self.size).rev() {
            if cid.is_free() {
                self.free.push(cid);
            }
        }
    }
    /// 同步到每一个FAT
    pub async fn sync_all(&mut self, bpb: &RawBPB, device: &dyn BlockDevice) {
        todo!()
    }
    pub fn show(&self, mut n: usize) {
        if n == 0 {
            n = usize::MAX;
        }
        for (i, &cid) in self.list.iter().take(self.size.min(n)).enumerate() {
            println!("{:>8X} -> {:>8X}", i, cid.0);
        }
    }
}
