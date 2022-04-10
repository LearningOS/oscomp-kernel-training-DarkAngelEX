use alloc::sync::Arc;

use crate::{
    access::{common::Fat32Enum, directory::Fat32Dir, AccessPath},
    block_cache::{manager::CacheManager, CacheRef},
    block_sync::SyncManager,
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo},
    mutex::SpinMutex,
    tools::{CID, SID},
    xdebug::assert_sie_closed,
    xerror::SysError,
    BlockDevice,
};

pub struct Fat32Manager {
    bpb: RawBPB,
    fsinfo: RawFsInfo,
    inner: SpinMutex<ManagerInner>,
    sync_manager: Option<Arc<SpinMutex<SyncManager>>>,
    device: Option<Arc<dyn BlockDevice>>,
}

pub struct ManagerInner {
    pub list: FatList,
    pub caches: CacheManager,
}

impl ManagerInner {
    pub fn list_caches(&mut self) -> (&mut FatList, &mut CacheManager) {
        (&mut self.list, &mut self.caches)
    }
}

impl Fat32Manager {
    pub const fn new(max_cache: usize) -> Self {
        Self {
            bpb: RawBPB::zeroed(),
            fsinfo: RawFsInfo::zeroed(),
            inner: SpinMutex::new(ManagerInner {
                list: FatList::empty(),
                caches: CacheManager::new(max_cache),
            }),
            sync_manager: None,
            device: None,
        }
    }
    pub async fn init(&mut self, device: Arc<dyn BlockDevice>) {
        assert_sie_closed();
        self.bpb.load(&*device).await;
        self.fsinfo.load(&self.bpb, &*device).await;
        self.inner.get_mut().list.load(&self.bpb, 0, &*device).await;
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
