use core::sync::atomic::{AtomicBool, AtomicUsize};

use alloc::sync::Arc;
use ftl_util::{
    error::SysError,
    fs::{Mode, OpenFlags, VfsInode},
};

use crate::fs::dev::shm::vfile::ShmVfile;

use self::vfile::ShmInode;

mod vfile;

pub fn init() {
    vfile::init()
}

pub async fn open_file(
    path: &[&str],
    flags: OpenFlags,
    _mode: Mode,
) -> Result<Arc<dyn VfsInode>, SysError> {
    stack_trace!();
    let (f_r, f_w) = flags.read_write()?;
    // println!("open_file {:?} flags: {:#x}, create: {}", stack, flags, flags.create());
    if flags.create() {
        match create_any(&path, flags.dir(), !f_w).await {
            Ok(_) => (),
            Err(SysError::EEXIST) => {
                if flags.dir() {
                    return Err(SysError::EISDIR);
                }
                delete_file(&path).await?;
                create_any(&path, false, !f_w).await?;
            }
            Err(e) => return Err(e),
        }
    }
    let inode = search_any(&path).await?;
    if f_w && !inode.writable() {
        return Err(SysError::EACCES);
    }
    Ok(Arc::new(ShmVfile {
        readable: AtomicBool::new(f_r),
        writable: AtomicBool::new(f_w),
        ptr: AtomicUsize::new(0),
        inode,
    }))
}

pub async fn unlink<'a>(path: &[&str], _flags: OpenFlags) -> Result<(), SysError> {
    stack_trace!();
    delete_any(path).await
}

async fn create_any(path: &[&str], is_dir: bool, read_only: bool) -> Result<(), SysError> {
    let (name, dir) = split_search_path(path).await?;
    match is_dir {
        true => dir.create_dir(name, read_only).await,
        false => dir.create_file(name, read_only).await,
    }
}

async fn delete_file(path: &[&str]) -> Result<(), SysError> {
    let (name, dir) = split_search_path(path).await?;
    dir.delete_file(name).await
}
async fn delete_dir(path: &[&str]) -> Result<(), SysError> {
    let (name, dir) = split_search_path(path).await?;
    dir.delete_dir(name).await
}
async fn delete_any(path: &[&str]) -> Result<(), SysError> {
    let (name, dir) = split_search_path(path).await?;
    dir.delete_any(name).await
}

async fn search_any(path: &[&str]) -> Result<Arc<ShmInode>, SysError> {
    let (name, dir) = match path.split_last() {
        Some((name, path)) => (name, search_dir(path).await?),
        None => return Ok(vfile::root()),
    };
    dir.search_any(name).await
}

async fn split_search_path<'a>(path: &[&'a str]) -> Result<(&'a str, Arc<ShmInode>), SysError> {
    match path.split_last() {
        Some((&name, path)) => {
            let dir = search_dir(path).await?;
            Ok((name, dir))
        }
        None => Err(SysError::ENOENT),
    }
}

async fn search_dir(mut path: &[&str]) -> Result<Arc<ShmInode>, SysError> {
    let mut cur = vfile::root();
    while let Some((xname, next_path)) = path.split_first() {
        path = next_path;
        cur = cur.search_dir(xname).await?;
    }
    Ok(cur)
}
