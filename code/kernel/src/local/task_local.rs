use alloc::sync::Arc;
use riscv::register::sstatus;

use crate::{
    memory::{self, PageTable},
    process::{proc_table, thread::Thread},
    tools::container::sync_unsafe_cell::SyncUnsafeCell,
    user::UserAccessStatus,
    xdebug::stack_trace::StackTrace,
};

/// 通过指针交换方式快速切换
///
/// 包含线程独立的信息
pub struct TaskLocal {
    pub user_access_status: UserAccessStatus, // 用户访问测试
    // 使用Option可以避免Arc Clone复制的CAS开销，直接移动到OutermostFuture。
    pub thread: Arc<Thread>,
    // 进程改变页表时需要同步到这里，更新回OutermostFuture
    pub page_table: Arc<SyncUnsafeCell<PageTable>>,
    // debug 栈追踪器
    pub stack_trace: StackTrace,
    pub sie_count: usize, // 不为0时关中断
    pub sum_count: usize, // 不为0时允许访问用户数据 必须关中断
}

impl TaskLocal {
    pub fn by_initproc() -> Self {
        proc_table::get_initproc()
            .alive_then(|a| Self {
                user_access_status: UserAccessStatus::Forbid,
                thread: a.threads.get_first().unwrap(),
                page_table: a.user_space.page_table_arc(),
                stack_trace: StackTrace::new(),
                sie_count: 0,
                sum_count: 0,
            })
            .unwrap()
    }
    pub(super) fn set_env(&self) {
        unsafe {
            if self.sie_count > 0 {
                sstatus::clear_sie();
            } else {
                sstatus::set_sie();
            }
            if self.sum_count > 0 {
                sstatus::set_sum();
            } else {
                sstatus::clear_sum();
            }
            self.page_table.get().using();
        }
    }
    pub(super) fn clear_env(&self) {
        unsafe {
            sstatus::clear_sie();
            sstatus::clear_sum();
            memory::set_satp_by_global();
        }
    }

    pub fn sum_inc(&mut self) {
        if self.sum_count == 0 {
            assert!(self.user_access_status.is_forbid());
            self.user_access_status.set_access();
            unsafe { sstatus::set_sum() };
        }
        self.sum_count += 1;
    }
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
    pub fn sie_inc(&mut self) {
        if self.sie_count == 0 {
            unsafe { sstatus::clear_sie() };
        }
        self.sie_count += 1;
    }
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
}
