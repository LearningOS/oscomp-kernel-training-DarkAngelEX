use core::{future::Future, pin::Pin};

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use ftl_util::{device::BlockDevice, error::SysError, utc_time::UtcTime};

use crate::{
    block::CacheManager,
    fat_list::FatList,
    inode::{inode_cache::InodeCache, manager::InodeManager, AnyInode, IID},
    layout::bpb::RawBPB,
    mutex::spin_mutex::SpinMutex,
    xdebug::assert_sie_closed,
    DirInode, FileInode,
};

pub struct Fat32Manager {
    pub(crate) bpb: RawBPB,
    pub(crate) list: FatList,
    pub(crate) caches: CacheManager,
    pub(crate) inodes: InodeManager,
    root_dir: Option<DirInode>,
    utc_time: Option<Box<dyn Fn() -> UtcTime + Send + Sync + 'static>>,
    rcu_handler: Option<Box<dyn Fn(Box<dyn Send + 'static>) + Send + Sync + 'static>>,
    rcu_pending: SpinMutex<Vec<Box<dyn Send + 'static>>>,
}

impl Fat32Manager {
    pub fn new(
        list_max_dirty: usize,
        list_max_cache: usize,
        block_max_dirty: usize,
        block_max_cache: usize,
        inode_target_free: usize,
    ) -> Self {
        Self {
            bpb: RawBPB::zeroed(),
            list: FatList::empty(list_max_dirty, list_max_cache),
            caches: CacheManager::new(block_max_dirty, block_max_cache),
            inodes: InodeManager::new(inode_target_free),
            root_dir: None,
            utc_time: None,
            rcu_handler: None,
            rcu_pending: SpinMutex::new(Vec::new()),
        }
    }
    pub async fn init(
        &mut self,
        device: Arc<dyn BlockDevice>,
        utc_time: Box<dyn Fn() -> UtcTime + Send + Sync + 'static>,
    ) {
        assert_sie_closed();
        self.bpb.load(&*device).await;
        self.list.init(&self.bpb, 0, device.clone()).await;
        self.caches.init(&self.bpb, device.clone()).await;
        self.inodes.init();
        self.utc_time = Some(utc_time);
        self.init_root();
    }
    pub async fn spawn_sync_task(
        &mut self,
        (concurrent_list, concurrent_cache): (usize, usize),
        spawn_fn: impl FnMut(Pin<Box<dyn Future<Output = ()> + Send + 'static>>)
            + Clone
            + Send
            + 'static,
    ) {
        let x = spawn_fn.clone();
        self.list.sync_task(concurrent_list, x).await;
        self.caches.sync_task(concurrent_cache, spawn_fn).await;
    }
    fn init_root(&mut self) {
        let cache = self
            .inodes
            .get_or_insert(IID::ROOT, || InodeCache::new_root(self));
        let raw_inode = unsafe { cache.get_root_inode() };
        self.root_dir.replace(DirInode::new(raw_inode));
    }
    /// 如果不初始化, 所有RCU内存将在析构时释放
    pub fn rcu_init(
        &mut self,
        rcu_handler: Box<dyn Fn(Box<dyn Send + 'static>) + Send + Sync + 'static>,
    ) {
        core::mem::take(&mut *self.rcu_pending.lock())
            .into_iter()
            .for_each(|a| rcu_handler(a));
        self.rcu_handler.replace(rcu_handler);
    }
    pub async fn search_any(&self, path: &[String]) -> Result<AnyInode, SysError> {
        let (name, dir) = match path.split_first() {
            Some((name, path)) => (name.as_str(), self.search_dir(path).await?),
            None => return Ok(AnyInode::Dir(self.root_dir())),
        };
        dir.search_any(self, name).await
    }
    pub async fn search_dir(&self, mut path: &[String]) -> Result<DirInode, SysError> {
        let mut cur = self.root_dir();
        while let Some((xname, next_path)) = path.split_first() {
            path = next_path;
            cur = cur.search_dir(self, xname).await?;
        }
        Ok(cur)
    }
    /// 搜索路径
    async fn split_search_path<'a>(
        &self,
        path: &'a [String],
    ) -> Result<(&'a str, DirInode), SysError> {
        match path.split_first() {
            Some((name, path)) => Ok((name, self.search_dir(path).await?)),
            None => Err(SysError::ENOENT),
        }
    }
    pub async fn search_file(&self, path: &[String]) -> Result<FileInode, SysError> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.search_file(self, name).await
    }
    /// 只能删除文件或空目录
    pub async fn delete_any(&self, path: &[String]) -> Result<(), SysError> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.delete_any(self, name).await
    }
    /// 只能删除空目录
    pub async fn delete_dir(&self, path: &[String]) -> Result<(), SysError> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.delete_dir(self, name).await
    }
    pub async fn delete_file(&self, path: &[String]) -> Result<(), SysError> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.delete_file(self, name).await
    }
    pub fn root_dir(&self) -> DirInode {
        self.root_dir.as_ref().unwrap().clone()
    }

    pub(crate) fn rcu_free(&self, src: impl Send + 'static) {
        debug_assert!(core::mem::size_of_val(&src) <= core::mem::size_of::<usize>());
        debug_assert!(core::mem::size_of_val(&src) == core::mem::align_of_val(&src));
        self.rcu_free_box(Box::new(src));
    }
    pub(crate) fn rcu_free_box(&self, src: Box<dyn Send + 'static>) {
        debug_assert!(core::mem::size_of_val(&*src) <= core::mem::size_of::<usize>());
        debug_assert!(core::mem::size_of_val(&*src) == core::mem::align_of_val(&*src));
        match self.rcu_handler.as_ref() {
            Some(f) => f(src),
            None => self.rcu_pending.lock().push(src),
        }
    }
    /// 返回UTC时间
    ///
    /// (year, mount, day), (hour, mount, second), millisecond
    pub(crate) fn utc_time(&self) -> UtcTime {
        self.utc_time.as_ref().unwrap()()
    }
}
