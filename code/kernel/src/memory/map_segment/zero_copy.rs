use core::mem::ManuallyDrop;

use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
    vec::Vec,
};
use ftl_util::faster;

use crate::{
    memory::{
        address::{PageCount, PhyAddrRef4K},
        allocator::frame::{self, global::FrameTracker, FrameAllocator},
    },
    sync::mutex::SpinLock,
};

use super::shared::SharedCounter;

pub struct SharePage(ManuallyDrop<SharedCounter>, PhyAddrRef4K);

impl Drop for SharePage {
    fn drop(&mut self) {
        unsafe {
            if ManuallyDrop::take(&mut self.0).consume() {
                frame::global::dealloc(self.1)
            }
        }
    }
}

impl Clone for SharePage {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1)
    }
}

impl SharePage {
    pub fn new(sc: SharedCounter, pa: PhyAddrRef4K) -> Self {
        Self(ManuallyDrop::new(sc), pa)
    }
    pub fn into_inner(mut self) -> (SharedCounter, PhyAddrRef4K) {
        let sc = unsafe { ManuallyDrop::take(&mut self.0) };
        let pa = self.1;
        core::mem::forget(self);
        (sc, pa)
    }
    pub fn addr(&self) -> PhyAddrRef4K {
        self.1
    }
    pub fn try_consume(self) -> Result<PhyAddrRef4K, Self> {
        if self.0.unique() {
            let pa = self.1;
            core::mem::forget(self);
            Ok(pa)
        } else {
            Err(self)
        }
    }
    pub fn release_by(mut self, allocator: &mut dyn FrameAllocator) {
        unsafe {
            if ManuallyDrop::take(&mut self.0).consume() {
                allocator.dealloc(self.1);
            }
        }
        core::mem::forget(self);
    }
    pub fn as_usize_array(&self) -> &[usize; 512] {
        self.1.as_usize_array()
    }
}

pub struct ZeroCopy {
    shared: BTreeMap<usize, SharePage>,
}

impl ZeroCopy {
    pub fn new() -> Self {
        Self {
            shared: BTreeMap::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.shared.is_empty()
    }
    pub fn contains(&self, offset: usize) -> bool {
        self.shared.contains_key(&offset)
    }
    pub fn insert(&mut self, offset: usize, sc: SharePage) {
        // 由于会在多核环境下使用, 因此允许释放
        let _ = self.shared.insert(offset, sc);
    }
    pub fn get(&self, offset: usize) -> Option<&SharePage> {
        self.shared.get(&offset)
    }
}

/// 用来在文件关闭的情况下缓存
static ZERO_COPY_SEARCH: SpinLock<BTreeMap<(usize, usize), Arc<SpinLock<ZeroCopy>>>> =
    SpinLock::new(BTreeMap::new());

pub fn get_zero_copy(dev: usize, ino: usize) -> Arc<SpinLock<ZeroCopy>> {
    ZERO_COPY_SEARCH
        .lock()
        .entry((dev, ino))
        .or_insert_with(|| Arc::new(SpinLock::new(ZeroCopy::new())))
        .clone()
}

pub fn remove_zero_copy(dev: usize, ino: usize) {
    let _ = ZERO_COPY_SEARCH.lock().remove(&(dev, ino));
}

/// 所有权页面缓存
///
/// 第三个成员是在路的页面的数量, 防止一堆请求堆起来让内存溢出
pub struct OwnCache(SharePage, Vec<FrameTracker>, usize);

const OWNER_HASH_SIZE: usize = 2048;
const CACHE_COUNT: usize = 2; // 每个共享页面的缓存数量

/// 所有权页面缓冲, 共享物理地址就是索引
pub struct OwnManager {
    requests: SpinLock<Option<VecDeque<SharePage>>>, // 复制请求
    table: [SpinLock<Vec<OwnCache>>; OWNER_HASH_SIZE], // 共享页面所有权缓冲
}

impl OwnManager {
    const NODE: SpinLock<Vec<OwnCache>> = SpinLock::new(Vec::new());
    pub const fn new() -> Self {
        Self {
            table: [Self::NODE; _],
            requests: SpinLock::new(None),
        }
    }
    fn index(page: &SharePage) -> usize {
        PageCount::page_floor(page.addr().into_usize()).0 % OWNER_HASH_SIZE
    }
    pub fn request_and_take_own(&self, page: &SharePage) -> Option<FrameTracker> {
        let index = Self::index(page);
        let mut ret = None;
        let mut req = true;
        let mut exist = false;
        let mut lk = self.table[index].lock();
        for cache in &mut *lk {
            if cache.0.addr() != page.addr() {
                continue;
            }
            exist = true;
            ret = cache.1.pop();
            if cache.1.len() + cache.2 >= CACHE_COUNT {
                req = false;
            } else {
                cache.2 += 1;
            }
            break;
        }
        if !exist {
            lk.push(OwnCache(page.clone(), Vec::new(), 1));
        }
        drop(lk);
        if req {
            self.requests
                .lock()
                .get_or_insert_with(|| VecDeque::new())
                .push_back(page.clone());
        }
        ret
    }
    /// 需要保证pa的内容和page完全相同
    pub fn insert_own_page(&self, page: &SharePage, pa: FrameTracker) {
        let index = Self::index(page);
        let mut lk = self.table[index].lock();
        for cache in &mut *lk {
            if cache.0.addr() != page.addr() {
                continue;
            }
            cache.1.push(pa);
            if cache.2 > 0 {
                cache.2 -= 1;
            }
            return;
        }
        // 找不到就会在这里释放内存
    }
    pub fn try_handle(&self) -> bool {
        if unsafe { self.requests.unsafe_get().is_none() } {
            return false;
        }
        if unsafe { self.requests.unsafe_get().as_ref().unwrap().is_empty() } {
            return false;
        }
        let req = self.requests.lock().as_mut().unwrap().pop_front();
        if let Some(req) = req {
            if let Ok(dst) = frame::global::alloc() {
                faster::page_copy(dst.data().as_usize_array_mut(), req.as_usize_array());
                self.insert_own_page(&req, dst);
                return true;
            }
        }
        false
    }
}

static OWN_MANAGER: OwnManager = OwnManager::new();

pub fn request_and_take_own(page: &SharePage) -> Option<FrameTracker> {
    OWN_MANAGER.request_and_take_own(page)
}

/// 不断生成共享页面
pub fn own_try_handle() -> bool {
    OWN_MANAGER.try_handle()
}
