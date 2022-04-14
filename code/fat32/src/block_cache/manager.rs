use alloc::collections::BTreeMap;

use crate::{
    block_cache::buffer::Buffer,
    layout::bpb::RawBPB,
    tools::{self, CID},
    xerror::SysError,
};

use super::{AccessID, Cache, CacheRef};

/// 此管理器仅用于获取块 不会进行任何读写操作 因此也不需要异步操作函数
pub struct CacheManager {
    access_sequence: BTreeMap<AccessID, *const Cache>, // 用来获取访问时间最长的块
    caches: BTreeMap<CID, Cache>,                      // 簇号 -> 块号
    alloc_id: AccessID,
    max_cache: usize,
}

impl CacheManager {
    pub const fn new(max_cache: usize) -> Self {
        // BTreeMap移动会改变元素位置, access_sequence使用的指针方法会爆炸!
        todo!();
        Self {
            access_sequence: BTreeMap::new(),
            caches: BTreeMap::new(),
            alloc_id: AccessID(0),
            max_cache,
        }
    }
    fn force_insert_cache(&mut self, mut cache: Cache, cid: CID) -> CacheRef {
        stack_trace!();
        let id = self.alloc_id.next();
        cache.init(cid, id);
        let cache = self.caches.try_insert(cid, cache).ok().unwrap();
        self.access_sequence.try_insert(id, cache).ok().unwrap();
        cache.get_cache_ref()
    }
    /// return Err when no buffer or no memory
    pub fn get_cache(&mut self, bpb: &RawBPB, cid: CID) -> Result<CacheRef, SysError> {
        stack_trace!();
        debug_assert!(cid.0 >= 2 && cid.0 < bpb.data_cluster_num as u32);
        if let Some(cache) = self.caches.get_mut(&cid) {
            // 存在缓存块 更新访问ID
            let new_id = self.alloc_id.next();
            let old_id = cache.update_id(new_id);
            self.access_sequence.remove(&old_id).unwrap();
            self.access_sequence.try_insert(new_id, cache).unwrap();
            return Ok(cache.get_cache_ref());
        }
        let cache = if self.caches.len() < self.max_cache {
            // 存在缓存块空间
            let buffer = Buffer::new(bpb.cluster_bytes)?;
            Cache::new(buffer)
        } else {
            // 转换一个缓存块
            let (aid, old_cid) = self
                .access_sequence
                .iter()
                .map(|(&aid, &c)| (aid, unsafe { &*c }))
                .find_map(|(aid, c)| c.no_owner().then_some((aid, c.cid())))
                .ok_or(SysError::ENOBUFS)?; // no buffer
            self.access_sequence.remove(&aid).unwrap();
            self.caches.remove(&old_cid).unwrap()
        };
        Ok(self.force_insert_cache(cache, cid))
    }
    /// 需要保证此缓存块的引用计数为0
    pub fn get_cache_init<T: Copy>(
        &mut self,
        bpb: &RawBPB,
        cid: CID,
        init_fn: impl FnOnce(&mut [T]) + 'static,
    ) -> Result<CacheRef, SysError> {
        stack_trace!();
        debug_assert!(cid.0 >= 2 && cid.0 < bpb.data_cluster_num as u32);
        if let Some(cache) = self.caches.get_mut(&cid) {
            // 存在缓存块 更新访问ID
            let new_id = self.alloc_id.next();
            let old_id = cache.update_id(new_id);
            self.access_sequence.remove(&old_id).unwrap();
            self.access_sequence.try_insert(new_id, cache).unwrap();
            cache.set_init_fn(move |buf| init_fn(tools::from_bytes_slice_mut(buf)));
            return Ok(cache.get_cache_ref());
        }
        let mut cache = if self.caches.len() < self.max_cache {
            // 存在缓存块空间
            let buffer = Buffer::new(bpb.cluster_bytes)?;
            Cache::new(buffer)
        } else {
            // 转换一个缓存块
            let (aid, old_cid) = self
                .access_sequence
                .iter()
                .map(|(&aid, &c)| (aid, unsafe { &*c }))
                .find_map(|(aid, c)| c.no_owner().then_some((aid, c.cid())))
                .ok_or(SysError::ENOBUFS)?; // no buffer
            self.access_sequence.remove(&aid).unwrap();
            self.caches.remove(&old_cid).unwrap()
        };
        cache.set_init_fn(|buf| init_fn(tools::from_bytes_slice_mut(buf)));
        Ok(self.force_insert_cache(cache, cid))
    }
    /// 尝试释放最久未访问的n个缓存块 返回释放的数量
    pub fn try_release_n(&mut self, n: usize) -> usize {
        stack_trace!();
        let mut cnt = 0;
        let caches = &mut self.caches;
        self.access_sequence.retain(|_, &mut cache| {
            let cache = unsafe { &*cache };
            if cnt >= n || !cache.no_owner() {
                return true;
            }
            caches.remove(&cache.cid()).unwrap(); // release memory by RAII there
            cnt += 1;
            false
        });
        cnt
    }
}
