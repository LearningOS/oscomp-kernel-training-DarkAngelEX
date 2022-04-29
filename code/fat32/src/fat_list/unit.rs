//! FAT链表扇区缓存
use core::cell::UnsafeCell;

use crate::{
    block::buffer::{Buffer, SharedBuffer},
    tools::{AID, CID},
    xerror::SysError,
};

// 此单元是链表的第几个扇区 ListUnit不变标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UnitID(pub u32);

/// 一个FAT表扇区
///
/// 看起来非常的unsafe, 但是从所有权来看ListUnit所有权属于inode,
/// inode修改fat链表必须要获取互斥锁, 因此当ListUnit改变时一定是持有互斥锁的,
/// 不同inode的修改处于不同的FAT链表, 绝对不会对同一块unit进行。
/// aid随便竞争, 反正不会导致系统爆炸
pub(crate) struct ListUnit {
    buffer: UnsafeCell<Buffer>, // cow内存
    aid: UnsafeCell<AID>,       // 访问ID
}

unsafe impl Send for ListUnit {}
unsafe impl Sync for ListUnit {}

impl ListUnit {
    pub fn new_uninit(sector_bytes: usize) -> Result<Self, SysError> {
        let buffer = Buffer::new(sector_bytes)?;
        Ok(Self {
            buffer: UnsafeCell::new(buffer),
            aid: UnsafeCell::new(AID(0)),
        })
    }
    pub fn init_load(&mut self) -> &mut [u8] {
        self.buffer.get_mut().access_rw().unwrap()
    }
    /// 此函数不更新aid
    pub fn raw_get(&self, off: usize) -> CID {
        unsafe { (&mut *self.buffer.get()).access_ro()[off] }
    }
    pub fn get(&self, off: usize, aid: AID) -> CID {
        unsafe {
            *self.aid.get() = aid;
            (&mut *self.buffer.get()).access_ro()[off]
        }
    }
    pub fn update_aid(&self, aid: AID) {
        // 不使用原子操作带来的违例不影响稳定性
        unsafe { *self.aid.get() = aid };
    }
    pub fn aid(&self) -> AID {
        unsafe { *self.aid.get() }
    }
    /// 只有manager可以操作此函数
    ///
    /// 操作manager必须持有list的排他锁
    pub unsafe fn set(&self, index: usize, cid: CID) -> Result<(), SysError> {
        (&mut *self.buffer.get()).access_rw()?[index] = cid;
        Ok(())
    }
    /// 此函数没有加锁
    pub unsafe fn to_unique(&self) -> Result<(), SysError> {
        (&mut *self.buffer.get()).access_rw::<u8>()?;
        Ok(())
    }
    pub fn buffer_ro(&self) -> &[CID] {
        unsafe { (&*self.buffer.get()).access_ro() }
    }
    pub fn buffer_rw(&mut self) -> Result<&mut [CID], SysError> {
        unsafe { (&mut *self.buffer.get()).access_rw() }
    }
    pub fn shared(&self) -> SharedBuffer {
        unsafe { (&mut *self.buffer.get()).share() }
    }
}
