//!
//! 高速目录项缓存
//!
//! dentry不会缓存整个目录树, 按需回收
//!
//! 每个dentry都持有父目录的强引用, 父目录只持有强引用计数不为0的子文件
//!
//!

use core::{
    ops::Deref,
    ptr::NonNull,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};

use alloc::{
    boxed::Box,
    collections::BTreeMap,
    sync::{Arc, Weak},
};
use ftl_util::{
    list::InListNode,
    rcu::{RcuCollect, RcuWraper},
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{
    hash_name::{AllHash, HashName, NameHash},
    inode::VfsInode,
    mount::Mount,
};

use self::lru_queue::LRUQueue;

mod index;
mod lru_queue;
pub mod manager;

pub struct Dentry {
    cache: Box<DentryCache>,
}

impl Deref for Dentry {
    type Target = DentryCache;
    fn deref(&self) -> &Self::Target {
        &self.cache
    }
}

impl Drop for Dentry {
    fn drop(&mut self) {
        let lru = unsafe { &*self.lru };
    }
}

inlist_access!(DentryHashNode, DentryCache, hash_node);
inlist_access!(DentryLruNode, DentryCache, lru_node);
inlist_access!(pub DentryFsspNode, DentryCache, fssp_node);
inlist_access!(DentrySubNode, DentryCache, sub_node);
/// 禁止改变父节点指针
///
/// 持有父目录所有权, 只能回收叶节点
pub struct DentryCache {
    closed: AtomicBool,
    using: RcuWraper<Weak<Dentry>>,
    name: HashName,
    parent: Option<Arc<Dentry>>, // 只有根目录为 None, 将通过RCU释放一个Weak指针防止内存回收
    hash_node: InListNode<Self, DentryHashNode>, // 只能被索引器修改
    /// 如果未使用将处于LRU队列
    lru: *const LRUQueue,
    lru_node: InListNode<Self, DentryLruNode>, // 由LRU队列控制
    lru_own: Option<Box<Self>>,
    /// 文件系统上的链表节点
    fssp_node: InListNode<Self, DentryFsspNode>,
    /// 挂载点指针 如果为挂载点则为Some
    mount: RcuWraper<Option<NonNull<Mount>>>,
    /// inode睡眠锁访问序列号, 只能被inode修改(通过引用)
    inode_seq: AtomicUsize,
    inode: SpinMutex<Option<Arc<VfsInode>>, Spin>,
    /// RCU子目录链表 通过RCU管理
    sub_head: SpinMutex<InListNode<Self, DentrySubNode>, Spin>,
    sub_node: InListNode<Self, DentrySubNode>, // 此节点连接到父目录的sub_head
}

unsafe impl Send for DentryCache {}
unsafe impl Sync for DentryCache {}

impl DentryCache {
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    /// 这个函数调用时会持有LRU队列锁
    fn close_by_lru_0(&mut self) {
        debug_assert!(!self.closed());
        debug_assert_eq!(self.using.get_mut().strong_count(), 0);
        debug_assert!(self.mount.get_mut().is_none());
        debug_assert!(self.sub_head.get_mut().is_empty());
        debug_assert!(self.sub_node.is_empty());

        self.closed.store(true, Ordering::Release);
        self.lru_own.take().unwrap().rcu_drop();
    }
    /// 此函数在释放LRU队列锁后运行
    fn close_by_lru_1(&mut self) {
        // 利用RCU释放weak防止内存被释放
        if let Some(parnet) = self.parent.take() {
            Arc::downgrade(&parnet).rcu_drop();
        }
    }
    pub fn hash(&self) -> AllHash {
        self.name.all_hash()
    }
    pub fn parent(&self) -> Option<Arc<Dentry>> {
        // self.parent.write_lock().clone()
        todo!()
    }
    /// 如果缓存存在将生成一个所有权Dentry
    pub fn try_child(&self, name: &str, name_hash: NameHash) -> Option<Arc<Dentry>> {
        // Ok(Some(child))
        todo!()
    }
}
