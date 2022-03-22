use core::ops::Range;

use alloc::{boxed::Box, sync::Arc};

use crate::{
    memory::{
        address::{PageCount, UserAddr4K},
        allocator::frame,
        PageTable,
    },
    tools::container::range_map::RangeMap,
};

use super::UserAreaHandler;

pub struct SegmentManager {
    map: RangeMap<UserAddr4K, Box<dyn UserAreaHandler>>,
    alloc_n: PageCount,
}

unsafe impl Send for SegmentManager {}
unsafe impl Sync for SegmentManager {}

impl Drop for SegmentManager {
    fn drop(&mut self) {
        assert_eq!(self.alloc_n, PageCount(0));
    }
}

impl SegmentManager {
    pub const fn new() -> Self {
        Self {
            map: RangeMap::new(),
            alloc_n: PageCount(0),
        }
    }
    pub fn try_push(
        &mut self,
        range: Range<UserAddr4K>,
        handler: Box<dyn UserAreaHandler>,
    ) -> Result<&dyn UserAreaHandler, Box<dyn UserAreaHandler>> {
        self.map.try_insert(range, handler).map(|a| &**a)
    }
    ///split_l: take the left side of the range
    ///
    ///split_r: take the right side of the range
    pub fn replace_push(
        &mut self,
        range: Range<UserAddr4K>,
        handler: Box<dyn UserAreaHandler>,
        pt: &mut PageTable, // release: impl FnMut(Box<dyn UserAreaHandler>),
    ) {
        let mut dealloc_n = PageCount(0);
        self.map.replace(
            range,
            handler,
            |a, b| a.split_l(b),
            |a, b| a.split_r(b),
            |a| dealloc_n += a.release(pt),
        );
        debug_assert!(self.alloc_n >= dealloc_n);
        self.alloc_n -= dealloc_n;
    }
}
