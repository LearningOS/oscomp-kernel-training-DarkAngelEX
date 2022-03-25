use alloc::collections::BTreeSet;

use crate::{memory::address::PhyAddrRef4K, xdebug::FRAME_RELEASE_CHECK};

pub struct FrameDetector(BTreeSet<PhyAddrRef4K>);

impl FrameDetector {
    pub const fn new() -> Self {
        Self(BTreeSet::new())
    }
    pub fn dealloc_run(&mut self, addr: PhyAddrRef4K) {
        if FRAME_RELEASE_CHECK {
            let f = self.0.insert(addr);
            assert!(f, "{}", to_red!("double dealloc frame"));
        }
    }
    pub fn alloc_run(&mut self, addr: PhyAddrRef4K) {
        if FRAME_RELEASE_CHECK {
            self.0.remove(&addr);
        }
    }
}
