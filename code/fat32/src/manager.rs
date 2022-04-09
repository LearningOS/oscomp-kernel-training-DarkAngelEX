use core::future::Future;

use alloc::sync::Arc;

use crate::{
    block_cache::{manager::CacheManager, CacheRef},
    block_sync::{sync_loop::sync_loop, SyncManager},
    fat_list::FatList,
    layout::{bpb::RawBPB, fsinfo::RawFsInfo},
    mutex::{Mutex, MutexSupport},
    tools::CID,
    BlockDevice,
};

pub struct Fat32Manager<S: MutexSupport> {
    bpb: RawBPB,
    fsinfo: RawFsInfo,
    fat_list: Mutex<FatList, S>,
    cache_manager: Mutex<CacheManager<S>, S>,
    sync_manager: Option<Arc<Mutex<SyncManager, S>>>,
    device: Option<Arc<dyn BlockDevice>>,
}

impl<S: MutexSupport> Fat32Manager<S> {
    pub const fn new(max_cache: usize) -> Self {
        Self {
            bpb: RawBPB::zeroed(),
            fsinfo: RawFsInfo::zeroed(),
            fat_list: Mutex::new(FatList::empty()),
            cache_manager: Mutex::new(CacheManager::new(max_cache)),
            sync_manager: None,
            device: None,
        }
    }
    pub async fn init(&mut self, device: Arc<dyn BlockDevice>) {
        self.bpb.load(&*device).await;
        self.fsinfo.load(&self.bpb, &*device).await;
        self.fat_list.get_mut().load(&self.bpb, 0, &*device).await;
        self.sync_manager = Some(Arc::new(Mutex::new(SyncManager::new())));
        self.device = Some(device);
    }
    pub fn get_sync_task(&self) -> impl Future<Output = ()> + Send + 'static {
        sync_loop(self.sync_manager.as_ref().unwrap().clone())
    }
    pub fn device(&self) -> &Arc<dyn BlockDevice> {
        self.device.as_ref().unwrap()
    }
    pub fn get_block(&self, cid: CID) -> Result<CacheRef<S>, ()> {
        stack_trace!();
        self.cache_manager.lock().get_cache(&self.bpb, cid)
    }
    pub fn get_root_block(&self) -> Result<CacheRef<S>, ()> {
        stack_trace!();
        let cid = CID(self.bpb.root_cluster_id);
        self.get_block(cid)
    }
}
