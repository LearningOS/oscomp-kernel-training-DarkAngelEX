use alloc::sync::Arc;

use crate::{
    cache::{manager::CacheManager, CacheRef},
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
    device: Option<Arc<dyn BlockDevice>>,
}

impl<S: MutexSupport> Fat32Manager<S> {
    pub const fn new(max_cache: usize) -> Self {
        Self {
            bpb: RawBPB::zeroed(),
            fsinfo: RawFsInfo::zeroed(),
            fat_list: Mutex::new(FatList::empty()),
            cache_manager: Mutex::new(CacheManager::new(max_cache)),
            device: None,
        }
    }
    pub async fn init(&mut self, device: Arc<dyn BlockDevice>) {
        self.bpb.load(&*device).await;
        self.fsinfo.load(&*device).await;
        self.fat_list.get_mut().load(&self.bpb, 0, &*device).await;
        self.device = Some(device);
    }
    pub fn device(&self) -> &Arc<dyn BlockDevice> {
        self.device.as_ref().unwrap()
    }
    pub fn get_block(&self, cid: CID) -> Result<CacheRef<S>, ()> {
        stack_trace!();
        self.cache_manager.lock().get_cache(&self.bpb, cid)
    }
}
