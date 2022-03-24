use alloc::boxed::Box;

use crate::{
    memory::{address::UserAddr4K, map_segment::sc_manager::SCManager, PageTable},
    tools::{
        self, allocator::from_usize_allocator::LeakFromUsizeAllocator,
        container::range_map::RangeMap, range::URange, xasync::HandlerID, ForwardWrapper,
    },
};

use super::UserAreaHandler;

pub struct HandlerManager {
    map: RangeMap<UserAddr4K, Box<dyn UserAreaHandler>>,
}

unsafe impl Send for HandlerManager {}
unsafe impl Sync for HandlerManager {}

impl Drop for HandlerManager {
    fn drop(&mut self) {}
}

impl HandlerManager {
    pub const fn new() -> Self {
        Self {
            map: RangeMap::new(),
        }
    }
    pub fn clear(&mut self, release: impl FnMut(Box<dyn UserAreaHandler>, URange)) {
        self.map.clear(release);
    }
    pub fn try_push(
        &mut self,
        range: URange,
        handler: Box<dyn UserAreaHandler>,
    ) -> Result<&mut dyn UserAreaHandler, Box<dyn UserAreaHandler>> {
        self.map.try_insert(range, handler).map(|a| &mut **a)
    }
    ///split_l: take the left side of the range
    ///
    ///split_r: take the right side of the range
    pub fn replace_push(
        &mut self,
        range: URange,
        handler: Box<dyn UserAreaHandler>,
        release: impl FnMut(Box<dyn UserAreaHandler>, URange),
    ) {
        self.map.replace(
            range,
            handler,
            |a, b, r| a.split_l(b, r),
            |a, b, r| a.split_r(b, r),
            release,
        );
    }
    pub fn remove_range(
        &mut self,
        range: URange,
        release: impl FnMut(Box<dyn UserAreaHandler>, URange),
    ) {
        self.map.remove(
            range,
            |a, b, r| a.split_l(b, r),
            |a, b, r| a.split_r(b, r),
            release,
        )
    }
    pub fn get(&self, addr: UserAddr4K) -> Option<&dyn UserAreaHandler> {
        self.map.get(addr).map(|a| a.as_ref())
    }
    pub fn range_contain(&self, range: URange) -> Option<&dyn UserAreaHandler> {
        self.map.range_contain(range).map(|a| a.as_ref())
    }
    pub fn range_match(&self, range: URange) -> Option<&dyn UserAreaHandler> {
        self.map.range_match(range).map(|a| a.as_ref())
    }
    /// 内部值使用 box_clone 复制
    pub fn fork(&mut self) -> Self {
        Self {
            map: self.map.fork(|a| a.box_clone()),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = (URange, &dyn UserAreaHandler)> {
        self.map.iter().map(|(a, b)| (a, b.as_ref()))
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (URange, &mut dyn UserAreaHandler)> {
        self.map.iter_mut().map(|(a, b)| (a, b.as_mut()))
    }
}

fn auto_shard_release(pt: &mut PageTable, r: URange, h: &dyn UserAreaHandler, sm: &mut SCManager) {
    if h.shared_writable().is_none() {
        h.unmap(pt, r);
        return;
    }
    for addr in tools::range::ur_iter(r) {
        let pte = match pt.try_get_pte_user(addr) {
            Some(pte) => pte,
            None => continue,
        };
        if pte.shared() {
            if sm.remove_ua(addr) {
                h.unmap_ua(pt, addr);
            } else {
                pte.reset();
            }
        } else {
            h.unmap_ua(pt, addr);
        }
    }
}
