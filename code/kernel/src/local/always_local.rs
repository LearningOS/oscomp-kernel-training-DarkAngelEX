use riscv::register::sstatus;

use crate::{user::UserAccessStatus, xdebug::stack_trace::StackTrace};

pub struct AlwaysLocal {
    sie_count: usize,                         // 不为0时关中断
    sum_count: usize,                         // 不为0时允许访问用户数据 必须关中断
    pub user_access_status: UserAccessStatus, // 用户访问测试
    pub stack_trace: StackTrace,              // debug 栈追踪器
}

impl AlwaysLocal {
    pub fn new() -> Self {
        Self {
            sie_count: 0,
            sum_count: 0,
            user_access_status: UserAccessStatus::Forbid,
            stack_trace: StackTrace::new(),
        }
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
    // return true will open interrupt
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
