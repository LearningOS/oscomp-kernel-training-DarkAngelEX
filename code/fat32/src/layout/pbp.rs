use core::ops::Range;

use crate::tools::{SID, self};

/// BIOS Parameter Block
///
/// logical offset = 0x0B
pub struct RawBPB {
    sector_bytes: u16,              // 扇区字节数
    sector_per_cluster: u8,         // 每簇扇区数
    sector_reserved: u16,           // 保留扇区数 用来获取第一个FAT偏移值
    fat_num: u8,                    // FAT副本数 通常为2
    discard_root_entry_size: u16,   // 根目录项数 FAT32=0
    discard_small_sector_size: u16, // 小扇区数 FAT32=0
    media_descriptor: u8,           // 媒体描述符 启用
    discard_sector_per_fat: u16,    // 每FAT使用扇区数 FAT32=0
    sertor_per_track: u16,          // 每道磁头数
    head_num: u16,                  // 磁头数
    sector_hidden: u32,             // 隐藏扇区数 无分区时为0
    sector_total: u32,              // 总扇区数
    sector_per_fat: u32,            // 每FAT使用扇区数 FAT32使用
    extended_flag: u16,             // 扩展标志
    version: u16,                   // 文件系统版本
    root_cluster_id: u32,           // 根目录簇号 通常为2
    info_cluster_id: u16,           // 文件系统信息扇区号 通常为1
    buckup_cluster_id: u16,         // 备份引导扇区 通常为6
    reversed_0: [u8; 12],           // 0
    physical_drive_num: u8,         // 物理驱动器号 软盘为0x00, 硬盘为0x80
    reversed_1: u8,                 // 0
    extended_boot_signature: u8,    // 0x28/0x29
    volume_serial_number: u32,      // 格式化后随机产生
    volume_label: [u8; 11],         // 卷标 "NO NAME"
    system_id: [u8; 8],             // "FAT32"
}

impl RawBPB {
    pub fn load(&mut self, src: &[u8; 512]) {
        // 不直接加载是因为结构体可能不对齐/rust重排序结构体
        let mut offset: usize = 0x0B;
        macro_rules! load {
            ($v: expr) => {
                tools::load_fn(&mut $v, src, &mut offset);
            };
        }
        load!(self.sector_bytes);
        load!(self.sector_per_cluster);
        load!(self.sector_reserved);
        load!(self.fat_num);
        load!(self.discard_root_entry_size);
        load!(self.discard_small_sector_size);
        load!(self.media_descriptor);
        load!(self.discard_sector_per_fat);
        load!(self.sertor_per_track);
        load!(self.head_num);
        load!(self.sector_hidden);
        load!(self.sector_total);
        load!(self.sector_per_fat);
        load!(self.extended_flag);
        load!(self.version);
        load!(self.root_cluster_id);
        load!(self.info_cluster_id);
        load!(self.buckup_cluster_id);
        load!(self.reversed_0);
        load!(self.physical_drive_num);
        load!(self.reversed_1);
        load!(self.extended_boot_signature);
        load!(self.volume_serial_number);
        load!(self.volume_label);
        load!(self.system_id);
        debug_assert_eq!(offset, 0x5A);
    }
    pub fn fat1_raw_range(&self) -> Range<SID> {
        let start = self.sector_reserved as u32;
        SID(start)..SID(start + self.sector_per_fat)
    }
    pub fn data_raw_range(&self) -> Range<SID> {
        let start = self.sector_hidden
            + self.sector_reserved as u32
            + self.sector_per_fat * self.fat_num as u32;
        SID(start)..SID(self.sector_total)
    }
}
