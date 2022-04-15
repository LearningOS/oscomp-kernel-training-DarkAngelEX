use core::cell::UnsafeCell;

use crate::{mutex::rw_sleep_mutex::RwSleepMutex, tools::AID, xerror::SysError};

use super::buffer::{Buffer, SharedBuffer};

/// 缓存一个簇
pub struct Cache {
    aid: UnsafeCell<AID>,
    buffer: RwSleepMutex<Buffer>,
}

unsafe impl Send for Cache {}
unsafe impl Sync for Cache {}

impl Cache {
    pub fn new(buffer: Buffer) -> Self {
        Self {
            aid: UnsafeCell::new(AID(0)),
            buffer: RwSleepMutex::new(buffer),
        }
    }
    pub(super) fn init(
        &mut self,
        aid: AID,
        init: impl FnOnce(&mut [u8]) + 'static,
    ) -> Result<(), SysError> {
        *self.aid.get_mut() = aid;
        init(self.buffer.get_mut().access_rw()?);
        Ok(())
    }
    /// 以只读打开一个缓存块 允许多个进程同时访问
    pub async fn access_ro<T: Copy, V>(&self, op: impl FnOnce(&[T]) -> V) -> V {
        stack_trace!();
        op(self.buffer.shared_lock().await.access_ro())
    }
    /// 以读写模式打开一个缓存块
    pub async fn access_rw<T: Copy, V>(
        &self,
        op: impl FnOnce(&mut [T]) -> V,
    ) -> Result<V, SysError> {
        stack_trace!();
        Ok(op(self.buffer.unique_lock().await.access_rw()?))
    }
    /// 只有manager可以获取mut
    pub fn init_buffer<T: Copy>(&mut self) -> Result<&mut [T], SysError> {
        stack_trace!();
        self.buffer.get_mut().access_rw()
    }
    pub fn aid(&self) -> AID {
        unsafe { *self.aid.get() }
    }
    /// 更新访问时间, 返回旧的值用于manager中更新顺序
    ///
    /// 需要确保在manager加锁状态中调用此函数 (唯一获取&mut Cache的方式)
    pub fn update_aid(&self, new: AID) {
        unsafe { (*self.aid.get()) = new }
    }
    pub async fn shared(&self) -> SharedBuffer {
        self.buffer.unique_lock().await.share()
    }
}
