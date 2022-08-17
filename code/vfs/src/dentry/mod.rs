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
    error::{SysError, SysR},
    list::InListNode,
    rcu::{RcuCollect, RcuWraper},
    sync::{sleep_mutex::SleepMutex, spin_mutex::SpinMutex, Spin},
};

use crate::{
    fssp::Fssp,
    hash_name::{AllHash, HashName, NameHash},
    inode::VfsInode,
    mount::Mount,
    FsInode, PRINT_OP, RRINT_ELIMINATE,
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
        stack_trace!();
        // 加入LRU队列
        if PRINT_OP {
            println!("dentry drop: {} begin", self.cache.name());
        }
        let own = unsafe { ManuallyDrop::take(&mut self.cache) };
        unsafe {
            let ptr = &mut *Box::into_raw(own);
            let own = Box::from_raw(ptr);
            debug_assert!(self.cache.lru_own.is_none());
            debug_assert!(self.cache.lru_node.is_empty());
            ptr.fssp.as_mut().insert_dentry(&mut ptr.fssp_node, || ());
            ptr.lru
                .as_mut()
                .insert(&mut ptr.lru_node, || ptr.lru_own = Some(own));
            ptr.using.rcu_write(Weak::new());
        }
        if PRINT_OP {
            println!("dentry drop: {} end", self.cache.name());
        }
    }
}

impl Dentry {
    pub fn is_dir(&self) -> bool {
        self.cache.is_dir
    }
    pub fn new_vfs_root(dentrys: &DentryManager, fssp: NonNull<Fssp>) -> Arc<Self> {
        Self::new_root(dentrys, fssp, InodeS::None)
    }
    pub fn new_root(dentrys: &DentryManager, fssp: NonNull<Fssp>, root_inode: InodeS) -> Arc<Self> {
        let (lru, index) = (dentrys.lru_ptr(), dentrys.index_ptr());
        let hn = HashName::new(core::ptr::null(), "");
        DentryCache::new_inited(hn, true, None, root_inode, (lru, fssp, index), false)
    }
    /// 通过序列号可以在不持有睡眠锁的情况下进行缓存搜索
    ///
    /// 如果持有睡眠锁后序列号没有改变则直接进入磁盘搜素过程, 不再重复搜素缓存
    pub fn inode_seq(&self) -> usize {
        self.cache.inode_seq.load(Ordering::Acquire)
    }
    /// 如果缓存存在将生成一个所有权Dentry
    ///
    /// 优先查找自身的子文件链表, 如果不存在则进入索引器查找
    pub fn search_child_in_cache(&self, name: &str, name_hash: NameHash) -> Option<Arc<Dentry>> {
        stack_trace!();
        unsafe {
            // 当前活跃目录的RCU查找
            for x in self.cache.sub_head.unsafe_get().next_iter() {
                atomic::fence(Ordering::Acquire);
                if !x.name.name_same(name_hash, name) || x.closed() {
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

    /// 如果序列号匹配说明子目录缓存没有变化, 跳过缓存名字搜索
    ///
    /// 此函数会持有睡眠锁
    pub fn search_child_deep_fast(
        self: &Arc<Self>,
        name: &str,
        name_hash: NameHash,
        inode_seq: usize,
    ) -> SysR<Arc<Dentry>> {
        stack_trace!();
        debug_assert!(self.is_dir());
        let cache = self.cache.as_ref();
        let _lk = cache.dir_lock.try_lock().ok_or(SysError::EAGAIN)?;
        if inode_seq != self.inode_seq() {
            if let Some(d) = self.search_child_in_cache(name, name_hash) {
                return Ok(d);
            }
        }
        if cache.closed() {
            return Err(SysError::ENOENT);
        }
        let inode = cache.inode.lock().clone().into_inode()?;
        let new = inode.search_fast(name)?;
        let dentry = DentryCache::new_inited(
            HashName::new(self.as_ref(), name),
            new.is_dir(),
            Some(self.clone()),
            InodeS::Some(new),
            (cache.lru, cache.fssp, cache.index),
            true,
        );
        debug_assert!(inode_seq == self.inode_seq());
        self.cache.seq_increase();
        Ok(dentry)
    }
    /// 如果序列号匹配说明子目录缓存没有变化, 跳过缓存名字搜索
    ///
    /// 此函数会持有睡眠锁
    pub async fn search_child_deep(
        self: &Arc<Self>,
        name: &str,
        name_hash: NameHash,
        inode_seq: usize,
    ) -> SysR<Arc<Dentry>> {
        stack_trace!();
        debug_assert!(self.is_dir());
        let cache = self.cache.as_ref();
        let _lk = cache.dir_lock.lock().await;
        if inode_seq != self.inode_seq() {
            if let Some(d) = self.search_child_in_cache(name, name_hash) {
                return Ok(d);
            }
        }
        if cache.closed() {
            return Err(SysError::ENOENT);
        }
        let inode = cache.inode.lock().clone().into_inode()?;
        let new = inode.search(name).await?;
        let dentry = DentryCache::new_inited(
            HashName::new(self.as_ref(), name),
            new.is_dir(),
            Some(self.clone()),
            InodeS::Some(new),
            (cache.lru, cache.fssp, cache.index),
            true,
        );
        debug_assert!(inode_seq == self.inode_seq());
        self.cache.seq_increase();
        Ok(dentry)
    }
    /// 这个函数会持有睡眠锁
    pub async fn create(
        self: &Arc<Self>,
        name: &str,
        dir: bool,
        rw: (bool, bool),
    ) -> SysR<Arc<Dentry>> {
        stack_trace!();
        debug_assert!(self.is_dir());
        let _lk = self.cache.dir_lock.lock().await;
        if self.cache.closed() {
            return Err(SysError::ENOENT);
        }
        let inode = self.cache.inode.lock().clone().into_inode()?;
        let hash_name = HashName::new(self.as_ref(), name);
        let nh = hash_name.name_hash();
        if let Some(d) = self.search_child_in_cache(name, nh) {
            return Ok(d);
        }
        // 文件名查重将由create内部进行
        let vfsinode = inode.create(name, dir, rw).await?;
        let dentry = DentryCache::new_inited(
            hash_name,
            dir,
            Some(self.clone()),
            InodeS::Some(vfsinode),
            (self.cache.lru, self.cache.fssp, self.cache.index),
            true,
        );
        self.cache.seq_increase();
        Ok(dentry)
    }
    pub async fn place_inode(
        self: &Arc<Self>,
        name: &str,
        inode: Box<dyn FsInode>,
    ) -> SysR<Arc<Dentry>> {
        stack_trace!();
        debug_assert!(self.is_dir());
        debug_assert!(!inode.is_dir());
        let _lk = self.cache.dir_lock.lock().await;
        if self.cache.closed() {
            return Err(SysError::ENOENT);
        }
        let dinode = self.cache.inode.lock().clone().into_inode()?;
        let hash_name = HashName::new(self.as_ref(), name);
        let nh = hash_name.name_hash();
        if let Some(d) = self.search_child_in_cache(name, nh) {
            return Ok(d);
        }
        // 文件名查重将由create内部进行
        let vfsinode = dinode.place_inode(name, inode).await?;
        let dentry = DentryCache::new_inited(
            hash_name,
            false,
            Some(self.clone()),
            InodeS::Some(vfsinode),
            (self.cache.lru, self.cache.fssp, self.cache.index),
            true,
        );
        self.cache.seq_increase();
        Ok(dentry)
    }
    pub async fn unlink(&self, name: &str) -> SysR<()> {
        stack_trace!();
        debug_assert!(self.is_dir());
        let _lk = self.cache.dir_lock.lock().await;
        if self.cache.closed() {
            return Err(SysError::ENOENT);
        }
        let inode = self.cache.inode.lock().clone().into_inode()?;
        let hash_name = HashName::new(self, name);
        let nh = hash_name.name_hash();
        let mut release = true;
        if let Some(d) = self.search_child_in_cache(name, nh) {
            if d.is_dir() {
                return Err(SysError::EISDIR);
            }
            if !d.cache.closed() {
                let inode = d.cache.inode.lock().clone().into_inode()?;
                d.cache.close_and_detach_inode()?;
                inode.detach().await?;
                release = false;
            }
        }
        inode.unlink_child(name, release).await
    }
    pub async fn rmdir(&self, name: &str) -> SysR<()> {
        stack_trace!();
        debug_assert!(self.is_dir());
        let _lk = self.cache.dir_lock.lock().await;
        if self.cache.closed() {
            return Err(SysError::ENOENT);
        }
        let inode = self.cache.inode.lock().clone().into_inode()?;
        let hash_name = HashName::new(self, name);
        let nh = hash_name.name_hash();
        if let Some(d) = self.search_child_in_cache(name, nh) {
            // 删除子目录缓存, 如果子目录被占用直接失败
            if !d.is_dir() {
                return Err(SysError::ENOTDIR);
            }
            if !d.cache.closed() {
                let inode = d.cache.inode.lock().clone().into_inode()?;
                d.cache.close_and_detach_inode()?;
                inode.detach().await?;
            }
        }
        inode.rmdir_child(name).await
    }
}

/// 将inode状态分段, dentry中inode为Init时阻止unlink
///
/// Init状态下一定被Dentry持有, 因此不会被释放
#[derive(Clone)]
pub(crate) enum InodeS {
    /// 这个Inode正在初始化
    Init,
    ///
    Some(Arc<VfsInode>),
    /// 逻辑上不需要inode
    None,
    /// 已经被释放, 这个cache已经无效了
    Closed,
}

impl InodeS {
    pub fn into_inode(self) -> SysR<Arc<VfsInode>> {
        let e = match self {
            Self::Some(i) => return Ok(i),
            InodeS::Init => SysError::EBUSY,
            InodeS::None => SysError::ENOENT,
            InodeS::Closed => SysError::ENOENT,
        };
        Err(e)
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
    /// 指向持有cache的dentry, 修改它不需要锁, 因为只有三种互斥的修改情况:
    /// 1. dentry生成
    /// 2. dentry析构
    /// 3. dentry主动释放dentrycache (持有父目录的锁)
    using: RcuWraper<Weak<Dentry>>,
    pub name: HashName,
    is_dir: bool,
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
    lru_own: Option<Box<Self>>, // 只有处于LRU队列时, 这里才是Some, 否则被dentry持有所有权
    /// 文件系统上的链表节点
    fssp: NonNull<Fssp>,
    fssp_node: InListNode<Self, DentryFsspNode>, // 当存在于LRU队列时才会加入节点
    /// 挂载点指针 如果为挂载点则为Some
    pub mount: RcuWraper<Option<NonNull<Mount>>>,
    dir_lock: SleepMutex<(), Spin>,     // 目录操作会用到这个锁
    inode_seq: AtomicUsize,             // inode睡眠锁访问序列号, 和dir_lock构成广义序列锁
    pub inode: SpinMutex<InodeS, Spin>, // 被关闭或 detached 为 None
    /// RCU子目录链表 通过RCU管理
    sub_head: SpinMutex<InListNode<Self, DentrySubNode>, Spin>,
    sub_node: InListNode<Self, DentrySubNode>, // 此节点连接到父目录的sub_head
}

unsafe impl Send for DentryCache {}
unsafe impl Sync for DentryCache {}

impl DentryCache {
    /// 将自身加入索引器
    pub fn new_inited(
        name: HashName,
        is_dir: bool,
        parent: Option<Arc<Dentry>>,
        inode: InodeS,
        (lru, fssp, index): (NonNull<LRUQueue>, NonNull<Fssp>, NonNull<DentryIndex>),
        in_index: bool,
    ) -> Arc<Dentry> {
        let mut cache = Box::new(Self {
            closed: AtomicBool::new(false),
            using: RcuWraper::new(Weak::new()),
            name,
            is_dir,
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
            dir_lock: SleepMutex::new(()),
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
            // 设置弱指针
            *this.using.get_mut() = wd;
            // 加入索引器
            if this.in_index {
                this.index.as_mut().insert(this);
                debug_assert!(!this.index_node.is_empty());
            }
            // 加入父目录链表
            if let Some(p) = this.parent.as_ref() {
                p.cache.sub_head.lock().push_prev(&mut this.sub_node);
            }
        }
        d
    }
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub fn name(&self) -> Arc<str> {
        self.name.name()
    }
    /// 这个序列号将在子目录缓存增加东西后调用, 减少不需要
    ///
    /// 这个函数没有锁!! 逻辑上需要持有自旋锁才能修改
    pub fn seq_increase(&self) {
        let a = self.inode_seq.load(Ordering::Acquire);
        self.inode_seq.store(a.wrapping_add(1), Ordering::Release);
    }
    /// 这个函数调用时会持有LRU队列锁
    ///
    /// 释放约束: 如果不存在外部引用则下面的全都没有
    fn close_by_lru_0(&mut self) {
        stack_trace!();
        if RRINT_ELIMINATE {
            println!("close by lru: {}", self.name());
        }
        debug_assert!(self.using.get_mut().strong_count() == 0);
        debug_assert!(self.mount.get_mut().is_none());
        debug_assert!(self.lru_own.is_some());
        debug_assert!(self.sub_head.get_mut().is_empty());
        if self.in_index {
            debug_assert!(!self.index_node.is_empty());
        }
        // debug_assert!(!self.closed()); // 被提前关闭了
        self.closed.store(true, Ordering::Release);
        // unwrap确认所有权存在
        self.lru_own.take().unwrap().rcu_drop(); // 在所有核经过await后释放
    }
    /// 此函数在释放LRU队列锁后运行
    fn close_by_lru_1(&mut self) {
        stack_trace!();
        unsafe {
            self.fssp.as_mut().remove_dentry(&mut self.fssp_node);
        }
        // 利用RCU释放weak防止内存被释放
        if let Some(parnet) = self.parent.take() {
            Arc::downgrade(&parnet).rcu_drop();
            let _lk = parnet.cache.sub_head.lock();
            self.sub_node.pop_self_rcu();
        }
        // 移除索引
        if self.in_index {
            unsafe { self.index.as_mut().remove(self) };
            self.in_index = false;
        }
        *self.inode.lock() = InodeS::Closed;
    }
    /// 此函数将使此缓存无效, 且inode将增加析构时释放标记
    ///
    /// 并发安全保证: 必须持有父目录睡眠锁调用此函数, 这个路径的dentry不会处于LRU队列
    fn close_and_detach_inode(&self) -> SysR<()> {
        debug_assert!(!self.closed());
        // 这条路径释放的cache不可能在LRU队列中
        debug_assert!(self.lru_node.is_empty());
        debug_assert!(self.lru_own.is_none());

        if self.is_dir {
            // 禁止释放挂载点或根目录
            if self.mount.rcu_read().is_some() || self.parent.is_none() {
                return Err(SysError::EBUSY);
            }
            if !self.sub_head.lock().is_empty() {
                return Err(SysError::ENOTEMPTY);
            }
        }

        self.closed.store(true, Ordering::Release);
        unsafe {
            self.using.rcu_write(Weak::new());
            let this = self.this_mut();
            if this.in_index {
                (*this.index.as_ptr()).remove(this);
                this.in_index = false;
            }
            *self.inode.lock() = InodeS::Closed;
        }
        Ok(())
    }
    pub fn hash(&self) -> AllHash {
        self.name.all_hash()
    }
    pub fn parent(&self) -> Option<Arc<Dentry>> {
        self.parent.clone()
    }
    #[allow(clippy::mut_from_ref)]
    #[allow(clippy::cast_ref_to_mut)]
    unsafe fn this_mut(&self) -> &mut Self {
        &mut *(self as *const _ as *mut Self)
    }
    /// 返回None说明这个缓存块已经无效了
    pub fn take_dentry(&self) -> Option<Arc<Dentry>> {
        stack_trace!();
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
                let a = (*self.lru.as_ptr()).lock_run(|cur| -> Ret {
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
                    debug_assert!(!this.lru_node.is_empty());
                    this.lru_node.pop_self();
                    *cur -= 1;
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
