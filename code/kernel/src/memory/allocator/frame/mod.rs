//! frame allocator which can be used in stack.

use alloc::{boxed::Box, vec::Vec};
use ftl_util::rcu::RcuCollect;

use crate::{
    memory::{self, address::PhyAddrRef4K},
    tools::{allocator::TrackerAllocator, error::FrameOOM},
};

use self::global::FrameTracker;

pub mod global;
pub mod iter;
mod list;

pub trait FrameAllocator = TrackerAllocator<PhyAddrRef4K, FrameTracker>;

#[inline]
pub fn default_allocator() -> impl FrameAllocator {
    // GlobalRefFrameAllocator::new()
    XFrameAllocator::new()
}

/// 最原始的帧分配器, 等效直接从全局管理器分配帧
struct GlobalRefFrameAllocator;

impl GlobalRefFrameAllocator {
    pub fn new() -> Self {
        Self
    }
}

impl TrackerAllocator<PhyAddrRef4K, FrameTracker> for GlobalRefFrameAllocator {
    fn alloc(&mut self) -> Result<FrameTracker, FrameOOM> {
        global::alloc()
    }
    unsafe fn dealloc(&mut self, value: PhyAddrRef4K) {
        global::dealloc(value)
    }
    fn alloc_directory(&mut self) -> Result<FrameTracker, FrameOOM> {
        global::alloc_directory()
    }
    unsafe fn dealloc_directory(&mut self, value: PhyAddrRef4K) {
        global::dealloc_directory(value)
    }
}

const CACHE_FRAME: usize = 10;

/// 以缓冲方式分配帧, 而释放内存将以RCU释放
///
/// RCU释放的内存可以保证这些帧都已经被刷下来了
struct XFrameAllocator {
    alloc: Vec<PhyAddrRef4K>,
    release: Vec<PhyAddrRef4K>,
}

impl XFrameAllocator {
    pub fn new() -> Self {
        Self {
            alloc: Vec::new(),
            release: Vec::new(),
        }
    }
}

impl TrackerAllocator<PhyAddrRef4K, FrameTracker> for XFrameAllocator {
    fn alloc(&mut self) -> Result<FrameTracker, FrameOOM> {
        unsafe {
            if let Some(v) = self.alloc.pop() {
                return Ok(FrameTracker::new(v));
            }
            self.alloc.resize(CACHE_FRAME, PhyAddrRef4K::from_usize(0));
            match global::alloc_iter(self.alloc.iter_mut()) {
                Ok(()) => {
                    let p = self.alloc.pop().unwrap();
                    Ok(FrameTracker::new(p))
                }
                Err(e) => {
                    self.alloc.clear();
                    Err(e)
                }
            }
        }
    }
    unsafe fn dealloc(&mut self, value: PhyAddrRef4K) {
        self.release.push(value)
    }
    fn alloc_directory(&mut self) -> Result<FrameTracker, FrameOOM> {
        global::alloc_directory()
    }
    unsafe fn dealloc_directory(&mut self, value: PhyAddrRef4K) {
        global::dealloc_directory(value)
    }
}

impl Drop for XFrameAllocator {
    fn drop(&mut self) {
        unsafe {
            global::dealloc_iter(self.alloc.iter());
            if !self.release.is_empty() {
                let v = Box::new(RcuDealloc(core::mem::take(&mut self.release)));
                memory::rcu::rcu_special_release(v.rcu_transmute());
            }
        }
    }
}

struct RcuDealloc(Vec<PhyAddrRef4K>);

impl Drop for RcuDealloc {
    fn drop(&mut self) {
        unsafe {
            global::dealloc_iter(self.0.iter());
        }
    }
}
