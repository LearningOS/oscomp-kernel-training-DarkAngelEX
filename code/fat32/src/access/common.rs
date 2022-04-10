use core::marker::PhantomData;

use crate::{
    block_cache::CacheRef, layout::bpb::RawBPB, manager::ManagerInner, mutex::SpinMutex,
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
                        self.cid = mi.list.get_next(cid);
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
    /// 无尽簇迭代器 永远不会返回None, 当磁盘满时返回Ok(Err(SysError::ENOSPC))
    ///
    /// 新分配的块将注册init_fn初始化函数 真正调用到这个块时将初始化
    pub fn cluster_alloc_iter<'a, T: Copy + 'a>(
        &'a self,
        bpb: &'a RawBPB,
        mi: &'a SpinMutex<ManagerInner>,
        init_fn: impl FnOnce(&mut [T]) + Copy + 'static,
    ) -> impl Iterator<Item = Result<CacheRef, SysError>> + 'a {
        return ClusterAllocIter {
            cur: self.cid,
            nxt: self.cid,
            bpb,
            mi,
            init_fn,
            _marker: PhantomData,
        };

        struct ClusterAllocIter<'a, T: Copy, F: FnOnce(&mut [T]) + Copy + 'static> {
            /// 上一次访问的簇号
            cur: CID,
            /// 下一次访问的簇号
            nxt: CID,
            bpb: &'a RawBPB,
            mi: &'a SpinMutex<ManagerInner>,
            init_fn: F,
            _marker: PhantomData<*const T>,
        }

        impl<'a, T: Copy, F: FnOnce(&mut [T]) + Copy + 'static> Iterator for ClusterAllocIter<'a, T, F> {
            type Item = Result<CacheRef, SysError>;
            fn next(&mut self) -> Option<Self::Item> {
                let mut mi = self.mi.lock();
                let (list, caches) = mi.list_caches();
                let cid = self.nxt;
                if !cid.is_next() {
                    // 分配新的块
                    debug_assert!(self.cur.is_next());
                    let mut need_clean = false;
                    let new = {
                        self.nxt = list.get_next(self.cur);
                        if self.nxt.is_last() {
                            need_clean = true;
                            match list.alloc_block_after(self.bpb, self.cur) {
                                Ok(cid) => cid,
                                Err(e) => return Some(Err(e)),
                            }
                        } else {
                            let cid = self.nxt;
                            self.nxt = list.get_next(cid);
                            cid
                        }
                    };
                    let ret = match need_clean {
                        true => caches.get_cache_init(self.bpb, cid, self.init_fn),
                        false => caches.get_cache(self.bpb, new),
                    };
                    return Some(ret);
                }
                let cache = caches.get_cache(&self.bpb, cid);
                // release cache_manager before lock list
                match cache {
                    Ok(cache) => {
                        self.nxt = list.get_next(cid);
                        Some(Ok(cache))
                    }
                    Err(e) => {
                        self.nxt.set_last();
                        Some(Err(e))
                    }
                }
            }
        }
    }
}
