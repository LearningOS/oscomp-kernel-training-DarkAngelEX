//! special fs

use core::sync::atomic::AtomicBool;

use alloc::boxed::Box;
use ftl_util::{
    list::InListNode,
    sync::{spin_mutex::SpinMutex, Spin},
};

use crate::{
    dentry::{DentryCache, DentryFsspNode},
    mount::{Mount, MountFsspNode},
};

pub trait Fs: Send + Sync + 'static {}
/// fs special
pub struct Fssp {
    closed: AtomicBool,
    mounts: SpinMutex<InListNode<Mount, MountFsspNode>, Spin>, // 持有此文件系统的挂载点, 最后一个挂载点将释放文件系统
    dentrys: SpinMutex<InListNode<DentryCache, DentryFsspNode>, Spin>, // 持有此文件系统的挂载点, 最后一个挂载点将释放文件系统
    fs: Box<dyn Fs>,
}

impl Fssp {
    pub fn insert_mount(&self, new: &mut InListNode<Mount, MountFsspNode>) {
        self.mounts.lock().push_prev(new)
    }
}
