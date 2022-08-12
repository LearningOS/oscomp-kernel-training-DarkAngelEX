use alloc::collections::BTreeMap;
use ftl_util::sync::shared_count::SharedCounter;

use crate::{
    memory::address::UserAddr4K,
    tools::{self, range::URange},
};

/// 管理共享页的引用计数, 原子计数实现
///
/// 此管理器的全部操作默认map中一定可以找到参数地址, 否则panic
pub struct SCManager(BTreeMap<UserAddr4K, SharedCounter>);

impl Drop for SCManager {
    fn drop(&mut self) {
        assert!(self.0.is_empty());
    }
}

impl SCManager {
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// 初始化引用计数为 2, 返回的会传给目标空间的insert_by
    ///
    /// 如果已经存在了将panic
    pub fn insert_clone(&mut self, ua: UserAddr4K) -> SharedCounter {
        stack_trace!();
        let (a, b) = SharedCounter::new_dup();
        self.0.try_insert(ua, a).ok().unwrap();
        b
    }
    /// 将 SharedCounter 加入共享管理器
    pub fn insert_by(&mut self, ua: UserAddr4K, x: SharedCounter) {
        self.0.try_insert(ua, x).ok().unwrap();
    }
    pub fn clone_ua(&mut self, ua: UserAddr4K) -> SharedCounter {
        stack_trace!();
        self.0.get(&ua).unwrap().clone()
    }
    /// 移除映射地址 并返回这是不是最后一个引用
    pub fn remove_ua(&mut self, ua: UserAddr4K) -> bool {
        self.0.remove(&ua).unwrap().consume()
    }
    /// 移除映射地址 并当此地址引用计数为 1 时返回 Ok(())
    pub fn remove_ua_result(&mut self, ua: UserAddr4K) -> Result<(), ()> {
        tools::bool_result(self.remove_ua(ua))
    }
    /// 当引用计数为 1 时置 0 并移除, 否则什么也不做
    ///
    /// 移除成功时返回 true
    pub fn try_remove_unique(&mut self, ua: UserAddr4K) -> bool {
        stack_trace!();
        let a = self.0.get(&ua).unwrap();
        // 观测到引用计数为 1 时一定是拥有所有权的, 不需要原子操作
        if a.unique() {
            let r = self.0.remove(&ua).unwrap().consume();
            debug_assert!(r);
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
        while let Some((&addr, _)) = self.0.range(range.clone()).next() {
            let rc = self.0.remove(&addr).unwrap();
            if rc.consume() {
                unique_release(addr)
            } else {
                shared_release(addr)
            }
        }
    }
    /// 无所有权释放全部的计数器
    ///
    /// 禁止存在页面的引用计数为 1 否则panic
    ///
    /// 此函数只在错误回退时使用
    pub fn check_remove_all(&mut self) {
        for (ua, sc) in core::mem::take(&mut self.0) {
            let r = sc.consume();
            debug_assert!(!r, "ua:{:?}", ua);
        }
    }
}
