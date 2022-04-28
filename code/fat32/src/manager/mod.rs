use alloc::{boxed::Box, sync::Arc};

use crate::{
    block::CacheManager,
    fat_list::FatList,
    inode::manager::InodeManager,
    layout::bpb::RawBPB,
    tools::{UtcTime, CID, SID},
    xdebug::assert_sie_closed,
    xerror::SysError,
    BlockDevice,
};

pub struct Fat32Manager {
    pub(crate) bpb: RawBPB,
    pub(crate) list: FatList,
    pub(crate) caches: CacheManager,
    pub(crate) inodes: InodeManager,
    device: Option<Arc<dyn BlockDevice>>,
    utc_time: Option<Box<dyn Fn() -> UtcTime + Send + 'static>>,
    rcu_handle: Option<Box<dyn Fn(Box<dyn Send + 'static>) + Send + 'static>>,
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
            device: None,
            utc_time: None,
            rcu_handle: None,
        }
    }
    pub async fn init(
        &mut self,
        device: Arc<dyn BlockDevice>,
        utc_time: Box<dyn Fn() -> UtcTime + Send + 'static>,
    ) {
        assert_sie_closed();
        self.bpb.load(&*device).await;
        self.list.init(&self.bpb, 0, device.clone()).await;
        self.caches.init(&self.bpb, device.clone()).await;
        self.inodes.init();
        self.device = Some(device);
        self.utc_time = Some(utc_time)
    }
    pub fn device(&self) -> &dyn BlockDevice {
        &**self.arc_device()
    }
    pub fn arc_device(&self) -> &Arc<dyn BlockDevice> {
        self.device.as_ref().unwrap()
    }
    /// 返回UTC时间
    ///
    /// (year, mount, day), (hour, mount, second), millisecond
    pub fn utc_time(&self) -> UtcTime {
        self.utc_time.as_ref().unwrap()()
    }
    // ==================================================================
    //                             私有操作
    // ==================================================================
    fn get_block(&self, cid: CID) -> Result<Arc<()>, SysError> {
        stack_trace!();
        todo!()
    }
    fn root_cid(&self) -> CID {
        CID(self.bpb.root_cluster_id)
    }
    fn transform(&self, cid: CID) -> SID {
        self.bpb.cid_transform(cid)
    }
}
