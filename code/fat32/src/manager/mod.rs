pub mod file;

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{
    async_tools::Async,
    device::BlockDevice,
    error::{SysError, SysR},
    time::UtcTime,
    xdebug,
};

use crate::{
    block::CacheManager,
    fat_list::FatList,
    inode::{inode_cache::InodeCache, manager::InodeManager, AnyInode, IID},
    layout::bpb::RawBPB,
    DirInode, FileInode,
};

pub struct Fat32Manager {
    pub(crate) bpb: RawBPB,
    pub(crate) list: FatList,
    pub(crate) caches: CacheManager,
    pub(crate) inodes: InodeManager,
    root_dir: Option<DirInode>,
    utc_time: Option<Box<dyn Fn() -> UtcTime + Send + Sync + 'static>>,
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
        }
    }
    pub async fn init(
        &mut self,
        device: Arc<dyn BlockDevice>,
        utc_time: Box<dyn Fn() -> UtcTime + Send + Sync + 'static>,
    ) {
        xdebug::assert_sie_closed();
        self.bpb.load(&*device).await;
        self.list.init(&self.bpb, 0, device.clone()).await;
        self.caches.init(&self.bpb, device.clone()).await;
        self.inodes.init();
        self.utc_time = Some(utc_time);
        self.init_root();
    }
    fn bpb(&self) -> &RawBPB {
        &self.bpb
    }
    pub async fn spawn_sync_task(
        &mut self,
        (concurrent_list, concurrent_cache): (usize, usize),
        spawn_fn: impl FnMut(Async<'static, ()>) + Clone + Send + 'static,
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
    pub async fn search_any(&self, path: &[&str]) -> SysR<AnyInode> {
        let (name, dir) = match path.split_last() {
            Some((name, path)) => (name, self.search_dir(path).await?),
            None => return Ok(AnyInode::Dir(self.root_dir())),
        };
        dir.search_any(self, name).await
    }
    pub async fn search_dir(&self, mut path: &[&str]) -> SysR<DirInode> {
        let mut cur = self.root_dir();
        while let Some((xname, next_path)) = path.split_first() {
            path = next_path;
            cur = cur.search_dir(self, xname).await?;
        }
        Ok(cur)
    }
    pub async fn search_file(&self, path: &[&str]) -> SysR<FileInode> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.search_file(self, name).await
    }
    pub async fn create_any(
        &self,
        path: &[&str],
        is_dir: bool,
        read_only: bool,
        hidden: bool,
    ) -> SysR<()> {
        let (name, dir) = self.split_search_path(path).await?;
        match is_dir {
            true => dir.create_dir(self, name, read_only, hidden).await,
            false => dir.create_file(self, name, read_only, hidden).await,
        }
    }
    /// 只能删除文件或空目录
    pub async fn delete_any(&self, path: &[&str]) -> SysR<()> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.delete_any(self, name).await
    }
    /// 只能删除空目录
    pub async fn delete_dir(&self, path: &[&str]) -> SysR<()> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.delete_dir(self, name).await
    }
    pub async fn delete_file(&self, path: &[&str]) -> SysR<()> {
        let (name, dir) = self.split_search_path(path).await?;
        dir.delete_file(self, name).await
    }
    /// 搜索路径
    async fn split_search_path<'a>(&self, path: &[&'a str]) -> SysR<(&'a str, DirInode)> {
        match path.split_last() {
            Some((&name, path)) => {
                let dir = self.search_dir(path).await?;
                Ok((name, dir))
            }
            None => Err(SysError::ENOENT),
        }
    }
    pub fn root_dir(&self) -> DirInode {
        self.root_dir.as_ref().unwrap().clone()
    }
    /// 返回UTC时间
    ///
    /// (year, mount, day), (hour, mount, second), millisecond
    pub(crate) fn utc_time(&self) -> UtcTime {
        self.utc_time.as_ref().unwrap()()
    }
}
