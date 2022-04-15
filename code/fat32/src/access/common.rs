use core::marker::PhantomData;

use crate::{
    layout::bpb::RawBPB, manager::ManagerInner, mutex::spin_mutex::SpinMutex,
    tools::CID, xerror::SysError,
};

use super::{directory::Fat32Dir, file::Fat32File};

pub enum Fat32Enum {
    File(Fat32File),
    Dir(Fat32Dir),
}
pub struct Fat32Common {
    cid: CID, // 此文件开始簇号
}

impl Fat32Common {
    pub fn new(cid: CID) -> Self {
        Self { cid }
    }
    pub fn start_cid(&self) -> CID {
        self.cid
    }
    /// 簇迭代器 返回可能未加载数据的块
    pub fn cluster_iter<'a>(
        &'a self,
        bpb: &'a RawBPB,
        mi: &'a SpinMutex<ManagerInner>,
    ) -> impl Iterator<Item = Result<CacheRef, SysError>> + 'a {
        return ClusterIter {
            cid: self.cid,
            bpb,
            mi,
        };

        struct ClusterIter<'a> {
            /// 下一次要访问时的簇号
            cid: CID,
            bpb: &'a RawBPB,
            mi: &'a SpinMutex<ManagerInner>,
        }

        impl<'a> Iterator for ClusterIter<'a> {
            type Item = Result<CacheRef, SysError>;
            fn next(&mut self) -> Option<Self::Item> {
                let cid = self.cid;
                if !cid.is_next() {
                    return None;
                }
                let mut mi = self.mi.lock();
                match mi.caches.get_cache(&self.bpb, cid) {
                    Ok(cache) => {
                        // self.cid = mi.list.get_next(cid);
                        todo!();
                        Some(Ok(cache))
                    }
                    Err(e) => {
                        self.cid.set_last();
                        Some(Err(e))
                    }
                }
            }
        }
    }
}
