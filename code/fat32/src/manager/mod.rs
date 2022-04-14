use alloc::sync::Arc;

use crate::{
    access::{common::Fat32Enum, directory::Fat32Dir, AccessPath},
    block_cache::{manager::CacheManager, CacheRef},
    block_sync::SyncManager,
    fat_list::FatList,
    inode::manager::InodeManager,
    layout::bpb::RawBPB,
    mutex::spin_mutex::SpinMutex,
    tools::{CID, SID},
    xdebug::assert_sie_closed,
    xerror::SysError,
    BlockDevice,
};

pub struct Fat32Manager {
    bpb: RawBPB,
    list: FatList,
    inner: SpinMutex<ManagerInner>,
    sync_manager: Option<Arc<SpinMutex<SyncManager>>>,
    device: Option<Arc<dyn BlockDevice>>,
}

pub struct ManagerInner {
    pub caches: CacheManager,
    pub inodes: InodeManager,
}

impl Fat32Manager {
    pub fn new(max_cache: usize, list_max_dirty: usize) -> Self {
        Self {
            bpb: RawBPB::zeroed(),
            list: FatList::empty(list_max_dirty, max_cache),
            inner: SpinMutex::new(ManagerInner {
                caches: CacheManager::new(max_cache),
                inodes: InodeManager::new(),
            }),
            sync_manager: None,
            device: None,
        }
    }
    pub async fn init(&mut self, device: Arc<dyn BlockDevice>) {
        assert_sie_closed();
        self.bpb.load(&*device).await;
        self.list.init(&self.bpb, 0, device.clone()).await;
        let inner = self.inner.get_mut();
        inner.inodes.init();
        self.sync_manager = Some(Arc::new(SpinMutex::new(SyncManager::new())));
        self.device = Some(device);
    }
    pub fn device(&self) -> &dyn BlockDevice {
        &**self.arc_device()
    }
    pub fn arc_device(&self) -> &Arc<dyn BlockDevice> {
        self.device.as_ref().unwrap()
    }
    pub async fn create(&self, path: &AccessPath) -> Result<Fat32Enum, SysError> {
        assert_sie_closed();
        path.assert_create();
        todo!()
    }
    pub async fn access(&self, path: &AccessPath) -> Result<Fat32Enum, SysError> {
        assert_sie_closed();
        path.assert_access();
        todo!()
    }
    pub async fn delete(&self, path: &AccessPath) -> Result<(), SysError> {
        assert_sie_closed();
        path.assert_delete();
        todo!()
    }
    // ==================================================================
    //                             私有操作
    // ==================================================================
    fn get_block(&self, cid: CID) -> Result<CacheRef, SysError> {
        stack_trace!();
        self.inner.lock().caches.get_cache(&self.bpb, cid)
    }
    fn root_cid(&self) -> CID {
        CID(self.bpb.root_cluster_id)
    }
    fn transform(&self, cid: CID) -> SID {
        self.bpb.cid_transform(cid)
    }
    async fn access_block_ro(
        &self,
        cache: &CacheRef,
        op: impl FnOnce(&[u8]),
    ) -> Result<(), SysError> {
        cache.get_ro(op, &self.bpb, self.device()).await
    }
    async fn walk_path(&self, path: &AccessPath) -> Result<Fat32Dir, SysError> {
        let root_cid = self.root_cid();
        let cur_dir = Fat32Dir::new(root_cid);
        todo!()
    }
}
