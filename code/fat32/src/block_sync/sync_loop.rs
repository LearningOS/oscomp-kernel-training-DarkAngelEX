use alloc::sync::Arc;

use crate::mutex::{Mutex, MutexSupport};

use super::SyncManager;

pub async fn sync_loop<S: MutexSupport>(_sync: Arc<Mutex<SyncManager, S>>) {
    todo!()
}
