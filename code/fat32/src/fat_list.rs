use alloc::{boxed::Box, collections::BTreeSet, sync::Arc, vec::Vec};

use crate::{
    block_sync::SyncTask,
    layout::bpb::RawBPB,
    tools::{self, CID, SID},
    BlockDevice,
};

/// 放置于内存的FAT表
///
/// 一个扇区放置128个CID
pub struct FatList {
    list: Vec<CID>,         // 整个FAT表
    free: Vec<CID>,         // 空闲FAT表分配器
    dirty: BTreeSet<usize>, // 已修改的FAT扇区相对start的偏移量
    size: usize,            // 簇数 list中超过size的将被忽略
}

impl FatList {
    pub const fn empty() -> Self {
        Self {
            list: Vec::new(),
            free: Vec::new(),
            dirty: BTreeSet::new(),
            size: 0,
        }
    }
    /// 加载第 n 个副本
    pub async fn load(&mut self, bpb: &RawBPB, n: usize, device: &dyn BlockDevice) {
        assert!(n < bpb.fat_num as usize);
        let start_sid = SID(bpb.fat_sector_start.0 as u32 + bpb.sector_per_fat * n as u32);
        self.size = bpb.data_cluster_num;
        let cid_per_sector = bpb.sector_bytes as usize / core::mem::size_of::<CID>();
        // list的长度 对齐到簇
        let len = bpb.sector_per_fat as usize * cid_per_sector;
        self.list.resize(len, CID(0));
        let buf = tools::to_bytes_slice_mut(&mut self.list);
        device.read_block(start_sid.0 as usize, buf).await.unwrap();
        for &cid in self.list.iter().take(self.size).rev() {
            if cid.is_free() {
                self.free.push(cid);
            }
        }
    }
    /// 表中某个簇号的偏移量
    fn get_offset_of_cid(bpb: &RawBPB, cid: CID) -> usize {
        cid.0 as usize >> (bpb.sector_bytes_log2 - core::mem::size_of::<u32>().log2())
    }
    /// 第offset个扇区对应的buffer
    fn get_buffer_of_sector<'a>(src: &'a [CID], bpb: &RawBPB, offset: usize) -> &'a [CID] {
        let per = bpb.sector_bytes as usize;
        &src[per * offset..per * (offset + 1)]
    }
    fn set_dirty(&mut self, bpb: &RawBPB, cids: &[CID]) {
        for &cid in cids {
            let offset = Self::get_offset_of_cid(bpb, cid);
            let _ = self.dirty.insert(offset);
        }
    }
    pub fn alloc_block(&mut self, bpb: &RawBPB) -> Option<CID> {
        let cid = self.free.pop()?;
        let target = &mut self.list[cid.0 as usize];
        debug_assert!(target.is_free());
        target.set_last();
        self.set_dirty(bpb, &[cid]);
        Some(cid)
    }
    pub fn alloc_block_after(&mut self, bpb: &RawBPB, cid: CID) -> Option<CID> {
        let dst = &mut self.list[cid.0 as usize];
        debug_assert!(dst.is_last());
        let new_cid = self.free.pop()?;
        dst.set_next(new_cid);
        let new = &mut self.list[new_cid.0 as usize];
        debug_assert!(new.is_free());
        new.set_last();
        self.set_dirty(bpb, &[cid, new_cid]);
        Some(new_cid)
    }
    fn synctask_generate(
        list: &[CID],
        start: SID,
        offset: usize,
        bpb: &RawBPB,
        device: &Arc<dyn BlockDevice>,
    ) -> Result<(SID, SyncTask), ()> {
        let mut buffer = unsafe {
            Box::try_new_uninit_slice(bpb.sector_bytes as usize)
                .map_err(|_| ())?
                .assume_init()
        };
        buffer.copy_from_slice(Self::get_buffer_of_sector(list, bpb, offset));
        let device = device.clone();
        let sid = SID(start.0 + offset as u32);
        Ok((
            sid,
            SyncTask::new(async move {
                let buffer = tools::to_bytes_slice(&*buffer);
                device.write_block(sid.0 as usize, buffer).await
            }),
        ))
    }
    /// 生成一个异步任务
    pub fn sync_all(
        &mut self,
        bpb: &RawBPB,
        starts: &[SID],
        device: &Arc<dyn BlockDevice>,
    ) -> Result<Vec<(SID, SyncTask)>, ()> {
        let mut tasks = Vec::new();
        for &start in starts {
            for &offset in &self.dirty {
                tasks.push(Self::synctask_generate(
                    &self.list, start, offset, bpb, device,
                )?);
            }
        }
        Ok(tasks)
    }
    /// 同步一个扇区 如果同步完成后不再有脏区域则返回true 否则返回false
    ///
    /// 出错返回操作成功的数量
    pub async fn sync_one(
        &mut self,
        bpb: &RawBPB,
        starts: &[SID],
        device: &Arc<dyn BlockDevice>,
    ) -> Result<Vec<(SID, SyncTask)>, ()> {
        let mut tasks = Vec::new();
        let offset = match self.dirty.first() {
            Some(&offset) => offset,
            None => return Ok(Vec::new()),
        };
        for &start in starts {
            tasks.push(Self::synctask_generate(
                &self.list, start, offset, bpb, device,
            )?);
        }
        self.dirty.pop_first().unwrap();
        Ok(tasks)
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
