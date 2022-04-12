use alloc::{
    boxed::Box,
    collections::BTreeMap,
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{mutex::SpinMutex, tools::CID};

use super::Fat32Inode;

/// 打开的文件将在此获取Inode, 如果Inode不存在则自动创建一个
///
/// Inode析构将抹去这里的记录
pub struct InodeManager {
    map: BTreeMap<CID, Weak<Fat32Inode>>, // 使用Weak来让析构函数在外部工作
    release_set: Option<Arc<SpinMutex<Vec<CID>>>>,
}

impl InodeManager {
    pub const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            release_set: None,
        }
    }
    pub fn init(&mut self) {
        self.release_set = Some(Arc::new(SpinMutex::new(Vec::new())));
    }
    fn release_fn(&self) -> Box<dyn FnOnce(CID)> {
        let weak = Arc::downgrade(self.release_set.as_ref().unwrap());
        Box::new(move |cid| {
            weak.upgrade().map(|a| a.lock().push(cid));
        })
    }
    /// 检查是否存在释放的inode. 由于inode析构函数在引用计数修改前执行，不能通过引用计数判断是否有效.
    pub fn release_check(&mut self) {
        let set = core::mem::take(&mut *self.release_set.as_mut().unwrap().lock());
        for cid in set.into_iter() {
            self.map.remove(&cid).unwrap();
        }
    }
    /// 如果inode未被使用则使用init_fn函数初始化
    pub fn alloc_inode<T>(
        &mut self,
        cid: CID,
        init_fn: impl FnOnce(&mut Fat32Inode) -> T, // 唯一获取可变引用的位置 cid和释放器已经设置
    ) -> (Arc<Fat32Inode>, Option<T>) {
        self.release_check();
        if let Some(inode) = self.map.get(&cid).and_then(|p| p.upgrade()) {
            return (inode, None);
        }
        let mut new_inode = Fat32Inode::new(cid);
        let v = init_fn(&mut new_inode);
        new_inode.set_clear_fn(self.release_fn());
        let p = Arc::new(new_inode);
        self.map.insert(cid, Arc::downgrade(&p));
        (p, Some(v))
    }
}
