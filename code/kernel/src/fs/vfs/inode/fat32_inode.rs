use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::{drivers, executor, timer, user::AutoSie};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};
use fat32::Fat32Manager;
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    error::{SysError, SysR, SysRet},
    fs::{
        stat::{Stat, S_IFDIR, S_IFREG},
        DentryType, File, OpenFlags, Seek,
    },
    time::{TimeSpec, UtcTime},
};

pub use fat32::AnyInode;

use super::VfsInode;


static mut MANAGER: Option<Fat32Manager> = None;

fn manager() -> &'static Fat32Manager {
    unsafe { MANAGER.as_ref().unwrap() }
}

pub async fn init() {
    stack_trace!();
    let _sie = AutoSie::new();
    unsafe {
        MANAGER = Some(Fat32Manager::new(100, 200, 100, 200, 100));
        let manager = MANAGER.as_mut().unwrap();
        manager
            .init(drivers::device().clone(), Box::new(|| UtcTime::base()))
            .await;
        manager
            .spawn_sync_task((8, 8), |f| executor::kernel_spawn(f))
            .await;
    }
}

pub async fn list_apps() {
    stack_trace!();
    let _sie = AutoSie::new();
    println!("/**** APPS ****");
    for (dt, name) in manager().root_dir().list(manager()).await.unwrap() {
        println!("{} {:?}", name, dt);
    }
    println!("**************/");
}
