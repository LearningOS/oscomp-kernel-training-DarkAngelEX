use core::{fmt::Display, mem::MaybeUninit};

use alloc::boxed::Box;
use ftl_util::device::BlockDevice;

use crate::tools::{self, CID, SID};

/// BIOS Parameter Block
///
/// logical offset = 0x0B
pub struct RawBPB {
    pub sector_bytes: u16,          // 扇区字节数
    pub sector_per_cluster: u8,     // 每簇扇区数
    sector_reserved: u16,           // 保留扇区数 用来获取第一个FAT偏移值
    pub fat_num: u8,                // FAT副本数 通常为2
    discard_root_entry_size: u16,   // 根目录项数 FAT32=0
    discard_small_sector_size: u16, // 小扇区数 FAT32=0
    media_descriptor: u8,           // 媒体描述符 弃用
    discard_sector_per_fat: u16,    // 每FAT使用扇区数 FAT32=0
    sertor_per_track: u16,          // 每道磁头数
    head_num: u16,                  // 磁头数
    sector_hidden: u32,             // 隐藏扇区数 无分区时为0 BPB扇区号
    sector_total: u32,              // 此分区总扇区数 从BPB到分区结束
    pub sector_per_fat: u32,        // 每FAT使用扇区数 FAT32使用
    extended_flag: u16,             // 扩展标志
    version: u16,                   // 文件系统版本
    pub root_cluster_id: u32,       // 根目录簇号 通常为2
    pub info_cluster_id: u16,       // 文件系统信息扇区号 通常为1
    buckup_cluster_id: u16,         // 备份引导扇区 通常为6
    reversed_0: [u8; 12],           // 0
    physical_drive_num: u8,         // 物理驱动器号 软盘为0x00, 硬盘为0x80
    reversed_1: u8,                 // 0
    extended_boot_signature: u8,    // 0x28/0x29
    volume_serial_number: u32,      // 格式化后随机产生
    volume_label: [u8; 11],         // 卷标 "NO NAME"
    system_id: [u8; 8],             // "FAT32"

    // 此部分为加载后自行计算
    pub sector_bytes_log2: u32,        // 每扇区字节数的log2
    pub cluster_bytes_log2: u32,       // 每扇区字节数的log2
    pub cluster_bytes: usize,          // 每簇字节数
    pub(crate) fat_sector_start: SID,  // FAT表开始扇区号
    pub(crate) data_sector_start: SID, // 数据区开始扇区号
    pub data_sector_num: usize,        // 数据区扇区数
    pub data_cluster_num: usize,       // 数据区簇数
}

impl RawBPB {
    pub const fn zeroed() -> Self {
        unsafe { MaybeUninit::zeroed().assume_init() }
    }
    pub async fn load(&mut self, device: &dyn BlockDevice) {
        let mut buf: Box<[u8]> =
            unsafe { Box::new_uninit_slice(device.sector_bytes()).assume_init() };
        let sector = device.sector_bpb();
        device.read_block(sector, &mut buf).await.unwrap();
        self.raw_load(&buf);
        assert_eq!(self.sector_hidden as usize, device.sector_bpb());
    }
    pub fn raw_load(&mut self, src: &[u8]) {
        // 不直接加载是因为结构体可能不对齐/rust重排序结构体
        let mut offset: usize = 0x0B;
        macro_rules! load {
            ($v: expr) => {
                tools::load_fn(&mut $v, src, &mut offset);
            };
        }
        assert!(src.len() >= 512);
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
        println!("{}", self);
        self.sector_bytes_log2 = self.sector_bytes.log2();
        self.cluster_bytes = self.sector_bytes as usize * self.sector_per_cluster as usize;
        self.cluster_bytes_log2 = self.cluster_bytes.log2();
        self.fat_sector_start = SID(self.sector_hidden + self.sector_reserved as u32);
        self.data_sector_start =
            SID(self.fat_sector_start.0 + self.sector_per_fat * self.fat_num as u32);
        self.data_cluster_num = (self.sector_hidden + self.sector_total - self.data_sector_start.0)
            as usize
            / self.sector_per_cluster as usize;
        self.data_sector_num = self.data_cluster_num * self.sector_per_cluster as usize;
    }
    pub(crate) fn cid_transform(&self, cid: CID) -> SID {
        debug_assert!(cid.0 >= 2);
        SID(self.data_sector_start.0 + (cid.0 - 2) * self.sector_per_cluster as u32)
    }
    /// (第几个簇, 簇内偏移)
    pub fn cluster_spilt(&self, offset: usize) -> (usize, usize) {
        (
            offset >> self.cluster_bytes_log2,
            offset & ((1 << self.cluster_bytes_log2) - 1),
        )
    }
}

impl Display for RawBPB {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // wl: writeln
        macro_rules! wl {
            ($name: expr, $field: ident) => {
                writeln!(f, concat!($name, "{}"), self.$field)?;
            };
        }
        // ws: writeln special
        macro_rules! ws {
            ($name: expr, $field: ident) => {
                writeln!(f, concat!($name, "{:?}"), self.$field)?;
            };
        }
        wl!("sector_bytes: - - - - - - ", sector_bytes);
        wl!("sector_per_cluster:       ", sector_per_cluster);
        wl!("sector_reserved:- - - - - ", sector_reserved);
        wl!("fat_num:                  ", fat_num);
        wl!("discard_root_entry_size:- ", discard_root_entry_size);
        wl!("discard_small_sector_size:", discard_small_sector_size);
        wl!("media_descriptor: - - - - ", media_descriptor);
        wl!("discard_sector_per_fat:   ", discard_sector_per_fat);
        wl!("sertor_per_track: - - - - ", sertor_per_track);
        wl!("head_num:                 ", head_num);
        wl!("sector_hidden:- - - - - - ", sector_hidden);
        wl!("sector_total:             ", sector_total);
        wl!("sector_per_fat: - - - - - ", sector_per_fat);
        wl!("extended_flag:            ", extended_flag);
        wl!("version:- - - - - - - - - ", version);
        wl!("root_cluster_id:          ", root_cluster_id);
        wl!("info_cluster_id:- - - - - ", info_cluster_id);
        wl!("buckup_cluster_id:        ", buckup_cluster_id);
        ws!("reversed_0: - - - - - - - ", reversed_0);
        wl!("physical_drive_num:       ", physical_drive_num);
        wl!("reversed_1: - - - - - - - ", reversed_1);
        wl!("extended_boot_signature:  ", extended_boot_signature);
        wl!("volume_serial_number: - - ", volume_serial_number);
        ws!("volume_label:             ", volume_label);
        ws!("system_id:- - - - - - - - ", system_id);
        Ok(())
    }
}
