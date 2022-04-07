use alloc::{collections::BTreeSet, vec::Vec};

use crate::tools::{CID, SID};

/// 放置于内存的FAT表
pub struct FatList {
    list: Vec<CID>,       // 整个FAT表
    free: Vec<CID>,       // 空闲FAT表分配器
    dirty: BTreeSet<SID>, // 已修改的FAT扇区
}
