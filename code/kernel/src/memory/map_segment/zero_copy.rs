use core::mem::ManuallyDrop;

use alloc::{collections::BTreeMap, sync::Arc};

use crate::{
    memory::{
        address::PhyAddrRef4K,
        allocator::frame::{self, FrameAllocator},
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
    set: BTreeMap<usize, SharePage>,
}

impl ZeroCopy {
    pub fn new() -> Self {
        Self {
            set: BTreeMap::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }
    pub fn contains(&self, offset: usize) -> bool {
        self.set.contains_key(&offset)
    }
    pub fn insert(&mut self, offset: usize, sc: SharePage) {
        // 由于会在多核环境下使用, 因此允许释放
        let _ = self.set.insert(offset, sc);
    }
    pub fn get(&self, offset: usize) -> Option<&SharePage> {
        self.set.get(&offset)
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
