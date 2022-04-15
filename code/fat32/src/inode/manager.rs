use alloc::{collections::BTreeMap, sync::Weak};

use crate::tools::CID;

use super::Fat32Inode;

/// 打开的文件将在此获取Inode, 如果Inode不存在则自动创建一个
///
/// Inode析构将抹去这里的记录
pub struct InodeManager {
    map: BTreeMap<CID, Weak<Fat32Inode>>, // 使用Weak来让析构函数在外部工作
}

impl InodeManager {
    pub const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }
    pub fn init(&mut self) {
        todo!()
    }
}
