use riscv::register::sstatus;

use crate::{user::UserAccessStatus, xdebug::stack_trace::StackTrace};

/// `AlwaysLocal`是会在不同线程之间切换的控制块, 每个线程都有各自的`AlwaysLocal`.
/// `AlwaysLocal`和`TaskLocal`的区别是调度态的CPU也会存在一个默认的`AlwaysLocal`,
/// 保证无论CPU运行在何种状态, 都可以获取到一个`AlwaysLocal`, 中断上下文不可以访问
/// `TaskLocal`, 但可以访问`AlwaysLocal`.
pub struct AlwaysLocal {
    sie_count: usize,                         // 不为0时关中断
    sum_count: usize,                         // 不为0时允许访问用户数据 必须关中断
    pub user_access_status: UserAccessStatus, // 用户访问测试
    pub stack_trace: StackTrace,              // debug 栈追踪器
}

impl AlwaysLocal {
    pub const fn new() -> Self {
        Self {
            sie_count: 0,
            sum_count: 0,
            user_access_status: UserAccessStatus::Forbid,
            stack_trace: StackTrace::new(),
        }
    }
    // swap_nonoverlapping 比 swap 更快
    pub fn swap(&mut self, other: &mut Self) {
        unsafe { core::ptr::swap_nonoverlapping(self, other, 1) }
    }
    #[inline(always)]
    pub fn sum_inc(&mut self) {
        if self.sum_count == 0 {
            assert!(self.user_access_status.is_forbid());
            self.user_access_status.set_access();
            unsafe { sstatus::set_sum() };
        }
        self.sum_count += 1;
    }
    #[inline(always)]
    pub fn sum_dec(&mut self) {
        assert!(self.sum_count != 0);
        self.sum_count -= 1;
        if self.sum_count == 0 {
            assert!(self.user_access_status.is_access());
            self.user_access_status.set_forbid();
            unsafe { sstatus::clear_sum() };
        }
    }
    pub fn sum_cur(&self) -> usize {
        self.sum_count
    }
    #[inline(always)]
    pub fn sie_inc(&mut self) {
        if self.sie_count == 0 {
            unsafe { sstatus::clear_sie() };
        }
        self.sie_count += 1;
    }
    #[inline(always)]
    pub fn sie_dec(&mut self) {
        assert!(self.sie_count != 0);
        self.sie_count -= 1;
        if self.sie_count == 0 {
            unsafe { sstatus::set_sie() };
        }
    }
    pub fn sie_cur(&self) -> usize {
        self.sie_count
    }
    // 这个函数会修改sum标志位并关闭中断, 结束临界区后再根据返回值设置中断标志位
    pub fn env_change(new: &mut Self, old: &mut Self) -> bool {
        unsafe {
            if (old.sum_cur() > 0) != (new.sum_cur() > 0) {
                if new.sum_cur() > 0 {
                    sstatus::set_sum();
                } else {
                    sstatus::clear_sum();
                }
            }
            new.sie_cur() == 0
        }
    }
}
