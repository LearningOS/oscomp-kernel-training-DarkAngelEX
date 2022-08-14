use alloc::collections::BTreeMap;

use crate::{
    memory::address::{PhyAddrRef4K, UserAddr4K},
    tools::range::URange,
};

use super::shared::{DecreaseCache, IncreaseCache, SharedGuard, SharedPage};

/// 管理共享页的引用计数, 原子计数实现, 这里每个页都在页表中存在映射
///
/// 此管理器的全部操作默认map中一定可以找到参数地址, 否则panic
pub struct SCManager {
    map: BTreeMap<UserAddr4K, SharedPage>,
    increase: IncreaseCache,
    decrease: DecreaseCache,
}

impl Drop for SCManager {
    fn drop(&mut self) {
        assert!(self.map.is_empty());
    }
}

impl SCManager {
    pub const fn new() -> Self {
        Self {
            map: BTreeMap::new(),
            increase: IncreaseCache::new(),
            decrease: DecreaseCache::new(),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    #[inline]
    pub fn flush(&mut self, guard: &mut SharedGuard, release: impl FnMut(PhyAddrRef4K)) {
        self.increase.flush(guard);
        self.decrease.flush(guard, release);
    }
    /// 没有需要提交的引用计数变更申请
    #[inline(always)]
    pub fn no_need_to_submit(&self) -> bool {
        self.increase.is_empty() && self.decrease.is_empty()
    }
    pub fn submit_size(&self) -> usize {
        self.increase.len() + self.decrease.len()
    }
    /// 伪原子地消耗一个`SharedCounter`, 并返回它是不是最后一个
    ///
    /// 如果返回了true, 由外界负责释放pa, 如果返回了false, 由管理系统自动释放pa
    #[must_use]
    fn consume(&mut self, sc: SharedPage) -> bool {
        match sc.try_consume() {
            Ok(()) => true,
            Err(sc) => {
                self.decrease.push(sc);
                false
            }
        }
    }
    /// 初始化引用计数为 2, 返回的会传给目标空间的insert_by
    ///
    /// 如果已经存在了将panic
    #[must_use]
    pub fn insert_dup(&mut self, ua: UserAddr4K, pa: PhyAddrRef4K) -> SharedPage {
        stack_trace!();
        let (a, b) = SharedPage::new_dup(pa);
        self.map.try_insert(ua, a).ok().unwrap();
        b
    }
    /// 将 SharedCounter 加入共享管理器
    pub fn insert_by(&mut self, ua: UserAddr4K, x: SharedPage) {
        self.map.try_insert(ua, x).ok().unwrap();
    }
    #[must_use]
    pub fn clone_ua(&mut self, ua: UserAddr4K) -> SharedPage {
        stack_trace!();
        self.map.get(&ua).unwrap().fork(&mut self.increase)
    }
    /// 移除映射地址 并返回这是不是最后一个引用
    ///
    /// 如果返回了ture, 由外部释放这块内存
    #[must_use]
    pub fn remove_ua(&mut self, ua: UserAddr4K) -> bool {
        let sc = self.map.remove(&ua).unwrap();
        self.consume(sc)
    }
    /// 当引用计数为 1 时置 0 并移除, 否则什么也不做
    ///
    /// 移除成功时返回 true, 外部可以获取所有权
    #[must_use]
    pub fn try_remove_unique(&mut self, ua: UserAddr4K) -> bool {
        stack_trace!();
        let a = self.map.get(&ua).unwrap();
        // 观测到引用计数为 1 时一定是拥有所有权的, 不需要原子操作
        if a.unique() {
            let sc = self.map.remove(&ua).unwrap();
            sc.try_consume().unwrap();
            true
        } else {
            false
        }
    }
    /// 移除范围内存在的每一个计数器, 并调用对应释放函数
    pub fn remove_release(
        &mut self,
        range: URange,
        mut shared_release: impl FnMut(UserAddr4K),
        mut unique_release: impl FnMut(UserAddr4K),
    ) {
        stack_trace!();
        while let Some((&addr, _)) = self.map.range(range.clone()).next() {
            let page = self.map.remove(&addr).unwrap();
            match page.try_consume() {
                Ok(()) => unique_release(addr),
                Err(page) => {
                    self.decrease.push(page);
                    shared_release(addr)
                }
            }
        }
    }
    /// 无所有权释放全部的计数器
    ///
    /// 禁止存在页面的引用计数为 1 否则panic
    ///
    /// 此函数只在错误回退时使用
    pub fn check_remove_all(&mut self) {
        for (ua, page) in core::mem::take(&mut self.map) {
            debug_assert!(!page.unique(), "ua:{:?}", ua);
            self.decrease.push(page);
        }
    }
}
