use alloc::boxed::Box;

use crate::{
    memory::address::{PageCount, UserAddr4K},
    tools::{container::range_map::RangeMap, range::URange},
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
    pub fn remove(&mut self, range: URange, release: impl FnMut(Box<dyn UserAreaHandler>, URange)) {
        self.map.remove(
            range,
            |a, b, r| a.split_l(b, r),
            |a, b, r| a.split_r(b, r),
            release,
        )
    }
    /// 位置必须位于某个段中间, 否则panic
    pub fn split_at(&mut self, p: UserAddr4K) {
        self.map.split_at(p, |a, p, r| a.split_r(p, r))
    }
    /// 如果某个段跨越了p, 将这个段切为两半
    pub fn split_at_maybe(&mut self, p: UserAddr4K) {
        self.map.split_at_maybe(p, |a, p, r| a.split_r(p, r))
    }
    /// 位置必须位于某个段中间, 否则panic
    pub fn split_at_run(
        &mut self,
        p: UserAddr4K,
        l_run: impl FnOnce(&mut dyn UserAreaHandler, URange),
        r_run: impl FnOnce(&mut dyn UserAreaHandler, URange),
    ) {
        self.map.split_at_run(
            p,
            |a, p, r| a.split_r(p, r),
            |h, r| l_run(h.as_mut(), r),
            |h, r| r_run(h.as_mut(), r),
        )
    }
    pub fn get(&self, addr: UserAddr4K) -> Option<&dyn UserAreaHandler> {
        self.map.get(addr).map(|a| a.as_ref())
    }
    pub fn get_mut(&mut self, addr: UserAddr4K) -> Option<&mut dyn UserAreaHandler> {
        self.map.get_mut(addr).map(|a| a.as_mut())
    }
    pub fn get_rv(&self, addr: UserAddr4K) -> Option<(URange, &dyn UserAreaHandler)> {
        self.map.get_rv(addr).map(|(r, a)| (r, a.as_ref()))
    }
    pub fn get_rv_mut(&mut self, addr: UserAddr4K) -> Option<(URange, &mut dyn UserAreaHandler)> {
        self.map.get_rv_mut(addr).map(|(r, a)| (r, a.as_mut()))
    }
    pub fn range_contain(&self, range: URange) -> Option<&dyn UserAreaHandler> {
        stack_trace!();
        self.map.range_contain(range).map(|a| a.as_ref())
    }
    pub fn range_match(&self, range: URange) -> Option<&dyn UserAreaHandler> {
        self.map.range_match(range).map(|a| a.as_ref())
    }
    pub fn find_free_range(&self, range: URange, n: PageCount) -> Option<URange> {
        self.map
            .find_free_range(range, n.0, |a, n| a.add_page(PageCount(n)))
    }
    pub fn free_range_check(&self, range: URange) -> Result<(), ()> {
        self.map.free_range_check(range)
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
    /// 返回起始位置在r中的段
    pub fn range(&self, r: URange) -> impl Iterator<Item = (URange, &dyn UserAreaHandler)> {
        self.map.range(r).map(|(a, b)| (a, b.as_ref()))
    }
    /// 返回起始位置在r中的段
    pub fn range_mut(
        &mut self,
        r: URange,
    ) -> impl Iterator<Item = (URange, &mut dyn UserAreaHandler)> {
        self.map.range_mut(r).map(|(a, b)| (a, b.as_mut()))
    }
}
