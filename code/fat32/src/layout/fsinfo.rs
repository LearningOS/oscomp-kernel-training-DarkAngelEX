use crate::tools;

/// 处于FAT32保留区
///
/// 通常位于逻辑扇区1
pub struct RawFsInfo {
    signature_head: u32, // must be 0x41615252
    // reversed_0: [u8; 480], 都为0
    signature_1: u32,     // must be 0x61417272
    cluster_free: u32,    // 空簇数
    cluster_next: u32,    // 下一个可用簇号 如果为 0xFFFFFFFF 将从2开始搜索
    reversed_0: [u8; 12], // 保留
    signature_trail: u32, // must be 0xAA550000
}

impl RawFsInfo {
    pub fn load(&mut self, src: &[u8; 512]) {
        let mut offset: usize = 0x0;
        macro_rules! load {
            ($v: expr) => {
                tools::load_fn(&mut $v, src, &mut offset);
            };
        }
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
