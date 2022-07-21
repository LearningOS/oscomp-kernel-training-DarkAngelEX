//!
//! 高速目录项缓存
//!
//! dentry不会缓存整个目录树, 按需回收
//!
//! 每个dentry都持有父目录的强引用, 父目录只持有强引用计数不为0的子文件
//!
//!

use core::{
    mem::ManuallyDrop,
    ptr::NonNull,
    sync::atomic::{self, AtomicBool, AtomicUsize, Ordering},
};

use alloc::{
    boxed::Box,
    sync::{Arc, Weak},
};
use ftl_util::{
    error::SysR,
    list::InListNode,
    rcu::{RcuCollect, RcuWraper},
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{
    fssp::Fssp,
    hash_name::{AllHash, HashName, NameHash},
    inode::VfsInode,
    mount::Mount,
};

use self::{index::DentryIndex, lru_queue::LRUQueue, manager::DentryManager};

mod index;
mod lru_queue;
pub mod manager;

pub(crate) struct Dentry {
    pub cache: ManuallyDrop<Box<DentryCache>>, // 保证在析构之前都为Some
}

impl Drop for Dentry {
    fn drop(&mut self) {
        // 加入LRU队列
        let own = unsafe { ManuallyDrop::take(&mut self.cache) };
        unsafe {
            let ptr = &mut *Box::into_raw(own);
            let own = Box::from_raw(ptr);
            ptr.fssp.as_mut().insert_dentry(&mut ptr.fssp_node, || ());
            ptr.lru
                .as_mut()
                .insert(&mut ptr.lru_node, || ptr.lru_own = Some(own));
            ptr.using.rcu_write(Weak::new());
        }
    }
}

impl Dentry {
    pub fn new_vfs_root(dentrys: &DentryManager, fssp: NonNull<Fssp>) -> Arc<Self> {
        Self::new_root(dentrys, fssp, None)
    }
    pub fn new_root(
        dentrys: &DentryManager,
        fssp: NonNull<Fssp>,
        root_inode: Option<Arc<VfsInode>>,
    ) -> Arc<Self> {
        let (lru, index) = (dentrys.lru_ptr(), dentrys.index_ptr());
        let hn = HashName::new(core::ptr::null(), "");
        DentryCache::new(hn, None, root_inode, lru, fssp, index, false)
    }
    pub fn inode_seq(&self) -> usize {
        self.cache.inode_seq.load(Ordering::Acquire)
    }
    /// 如果缓存存在将生成一个所有权Dentry
    ///
    /// 优先查找自身的子文件链表, 如果不存在则进入索引器查找
    pub fn search_child_in_cache(&self, name: &str, name_hash: NameHash) -> Option<Arc<Dentry>> {
        unsafe {
            // 当前活跃目录的RCU查找
            for x in self.cache.sub_head.unsafe_get().next_iter() {
                atomic::fence(Ordering::Acquire);
                if x.name.name_same(name_hash, name) {
                    continue;
                }
                if let Some(d) = x.take_dentry() {
                    return Some(d);
                }
            }
            // 索引器查找
            let cache = self.cache.index.as_ref().get(&HashName::new(self, name))?;
            cache.take_dentry()
        }
    }
    /// 如果序列号不匹配则会返回None, 需要重试
    pub async fn search_child_deep(
        &self,
        name: &str,
        name_hash: NameHash,
        inode_seq: usize,
    ) -> Option<SysR<Arc<Dentry>>> {
        todo!()
    }
}

inlist_access!(DentryIndexNode, DentryCache, index_node);
inlist_access!(pub DentryLruNode, DentryCache, lru_node);
inlist_access!(pub DentryFsspNode, DentryCache, fssp_node);
inlist_access!(DentrySubNode, DentryCache, sub_node);
/// 禁止改变父节点指针
///
/// 持有父目录所有权, 只能回收叶节点
pub(crate) struct DentryCache {
    closed: AtomicBool,
    using: RcuWraper<Weak<Dentry>>,
    name: HashName,
    /// 只有根目录为 None, 将通过RCU释放一个Weak指针防止内存回收
    ///
    /// 当cache存在时父目录一定不会被释放
    parent: Option<Arc<Dentry>>,
    /// 索引器指针
    index: NonNull<DentryIndex>, // 如果为None则不在索引器中
    index_node: InListNode<Self, DentryIndexNode>, // 只能被索引器修改
    in_index: bool,
    /// 如果未使用将处于LRU队列
    lru: NonNull<LRUQueue>,
    lru_node: InListNode<Self, DentryLruNode>, // 由LRU队列控制
    lru_own: Option<Box<Self>>,
    /// 文件系统上的链表节点
    fssp: NonNull<Fssp>,
    fssp_node: InListNode<Self, DentryFsspNode>, // 当存在于LRU队列时才会加入节点
    /// 挂载点指针 如果为挂载点则为Some
    pub mount: RcuWraper<Option<NonNull<Mount>>>,
    /// inode睡眠锁访问序列号, 只能被inode内部修改(通过引用)
    inode_seq: AtomicUsize,
    pub inode: SpinMutex<Option<Arc<VfsInode>>, Spin>,
    /// RCU子目录链表 通过RCU管理
    sub_head: SpinMutex<InListNode<Self, DentrySubNode>, Spin>,
    sub_node: InListNode<Self, DentrySubNode>, // 此节点连接到父目录的sub_head
}

unsafe impl Send for DentryCache {}
unsafe impl Sync for DentryCache {}

impl DentryCache {
    /// 将自身加入索引器
    pub fn new(
        name: HashName,
        parent: Option<Arc<Dentry>>,
        inode: Option<Arc<VfsInode>>,
        lru: NonNull<LRUQueue>,
        fssp: NonNull<Fssp>,
        index: NonNull<DentryIndex>,
        in_index: bool,
    ) -> Arc<Dentry> {
        let mut cache = Box::new(Self {
            closed: AtomicBool::new(false),
            using: RcuWraper::new(Weak::new()),
            name,
            parent,
            index,
            index_node: InListNode::new(),
            in_index,
            lru,
            lru_node: InListNode::new(),
            lru_own: None,
            fssp,
            fssp_node: InListNode::new(),
            mount: RcuWraper::new(None),
            inode_seq: AtomicUsize::new(0),
            inode: SpinMutex::new(inode),
            sub_head: SpinMutex::new(InListNode::new()),
            sub_node: InListNode::new(),
        });
        cache.index_node.init();
        cache.lru_node.init();
        cache.fssp_node.init();
        cache.sub_head.get_mut().init();
        cache.sub_node.init();
        let mut d = Arc::new(Dentry {
            cache: ManuallyDrop::new(cache),
        });
        let wd = Arc::downgrade(&d);
        unsafe {
            let this = &mut *Arc::get_mut_unchecked(&mut d).cache;
            *this.using.get_mut() = wd;
            if this.in_index {
                this.index.as_mut().insert(this);
            }
        }
        d
    }
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    /// 这个函数调用时会持有LRU队列锁
    ///
    /// 释放约束: 如果不存在外部引用则下面的全都没有
    fn close_by_lru_0(&mut self) {
        debug_assert!(!self.closed());
        debug_assert!(self.using.get_mut().strong_count() == 0);
        debug_assert!(self.mount.get_mut().is_none());
        debug_assert!(self.lru_own.is_some());
        debug_assert!(self.sub_head.get_mut().is_empty());
        debug_assert!(self.sub_node.is_empty());
        self.closed.store(true, Ordering::Release);
        // unwrap确认所有权存在
        self.lru_own.take().unwrap().rcu_drop(); // 在所有核经过await后释放
    }
    /// 此函数在释放LRU队列锁后运行
    fn close_by_lru_1(&mut self) {
        unsafe {
            self.fssp.as_mut().remove_dentry(&mut self.fssp_node);
        }
        // 利用RCU释放weak防止内存被释放
        if let Some(parnet) = self.parent.take() {
            Arc::downgrade(&parnet).rcu_drop();
        }
        // 移除索引
        if self.in_index {
            unsafe { self.index.as_mut().remove(self) };
        }
        *self.inode.lock() = None;
    }
    pub fn hash(&self) -> AllHash {
        self.name.all_hash()
    }
    pub fn parent(&self) -> Option<Arc<Dentry>> {
        self.parent.clone()
    }
    unsafe fn this_mut(&self) -> &mut Self {
        &mut *(self as *const _ as *mut Self)
    }
    /// 返回None说明这个缓存块已经无效了
    pub fn take_dentry(&self) -> Option<Arc<Dentry>> {
        let r = loop {
            if self.closed() {
                return None;
            }
            if let Some(d) = self.using.rcu_read().upgrade() {
                return Some(d);
            }
            enum Ret {
                End(Option<Arc<Dentry>>),
                Retry,
            }
            unsafe {
                let a = (*self.lru.as_ptr()).lock_run(|| -> Ret {
                    if self.closed() {
                        return Ret::End(None);
                    }
                    if let Some(d) = self.using.rcu_read().upgrade() {
                        return Ret::End(Some(d));
                    }
                    let this = self.this_mut();
                    // 争抢所有权
                    let cache = match this.lru_own.take() {
                        None => return Ret::Retry,
                        Some(p) => p,
                    };
                    this.lru_node.pop_self();
                    Ret::End(Some(Arc::new(Dentry {
                        cache: ManuallyDrop::new(cache),
                    })))
                });
                match a {
                    Ret::Retry => continue,
                    Ret::End(v) => break v,
                }
            }
        };
        match r {
            None => None,
            Some(d) => unsafe {
                let this = self.this_mut();
                (*self.fssp.as_ptr()).remove_dentry(&mut this.fssp_node);
                // 所有权可见
                self.using.rcu_write(Arc::downgrade(&d));
                Some(d)
            },
        }
    }
}
