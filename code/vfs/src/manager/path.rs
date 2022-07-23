use core::ptr::NonNull;

use alloc::sync::Arc;
use ftl_util::error::{SysError, SysR};

use crate::{
    dentry::{Dentry, InodeS},
    hash_name::HashName,
    mount::Mount,
    VfsFile, VfsManager,
};

#[derive(Clone)]
pub(crate) struct Path {
    pub mount: Option<NonNull<Mount>>,
    pub dentry: Arc<Dentry>,
}

unsafe impl Send for Path {}
unsafe impl Sync for Path {}

impl Path {
    pub(crate) fn inode_s(&self) -> InodeS {
        self.dentry.cache.inode.lock().clone()
    }
    fn is_vfs_root(&self) -> bool {
        if !self.is_fs_root() {
            return false;
        }
        let mut path = self.clone();
        path.run_mount_prev();
        path.mount.is_none() && path.is_fs_root()
    }
    fn is_fs_root(&self) -> bool {
        self.dentry.cache.parent().is_none()
    }
    fn run_mount_prev(&mut self) {
        loop {
            let mount = match self.mount {
                None => return, // root dentry
                Some(m) => m,
            };
            unsafe {
                // 如果当前目录就是挂载点的根目录就回退一级
                if !core::ptr::eq(self.dentry.as_ref(), mount.as_ref().root()) {
                    return;
                }
                self.mount = mount.as_ref().parent;
                self.dentry = mount.as_ref().locate_arc();
            }
        }
    }
    fn run_mount_next(&mut self) {
        loop {
            let mount = match *self.dentry.cache.mount.rcu_read() {
                None => return,
                Some(mount) => mount,
            };
            unsafe {
                self.mount = Some(mount);
                self.dentry = mount.as_ref().root_arc();
            }
        }
    }
    async fn search_child(&mut self, s: &str) -> SysR<()> {
        if name_invalid(s) || self.dentry.cache.closed() {
            return Err(SysError::ENOENT);
        }
        let name_hash = HashName::hash_name(s);
        let inode_seq = self.dentry.inode_seq();
        if let Some(next) = self.dentry.search_child_in_cache(s, name_hash) {
            self.dentry = next;
            return Ok(());
        }
        self.dentry = self
            .dentry
            .search_child_deep(s, name_hash, inode_seq)
            .await?;
        Ok(())
    }
    pub fn parent(&self) -> Option<Path> {
        let mut path = self.clone();
        path.run_mount_prev();
        let dentry = path.dentry.cache.parent()?;
        Some(Path {
            mount: path.mount,
            dentry,
        })
    }
}

impl VfsManager {
    /// 返回到达最后一个文件名的路径和文件名
    pub(crate) async fn walk_path<'a>(
        &self,
        (base, path_str): (SysR<Arc<VfsFile>>, &'a str),
    ) -> SysR<(Path, &'a str)> {
        let mut path = if is_absolute_path(path_str) {
            Path {
                mount: None,
                dentry: self.root.as_ref().unwrap().clone(),
            }
        } else {
            base?.path.clone()
        };
        let (path_str, name) = match path_str.rsplit_once(['/', '\\']) {
            Some((path, name)) => (path, name),
            None => ("", path_str),
        };
        for s in path_str.split(['/', '\\']).map(|s| s.trim()) {
            path = self.walk_name(path, s).await?;
        }
        path.run_mount_next();
        Ok((path, name))
    }
    pub(crate) async fn walk_name(&self, mut path: Path, name: &str) -> SysR<Path> {
        // 当前目录为根目录
        println!("walk_name: {} -> {}", path.dentry.cache.name(), name);
        if path.is_vfs_root() {
            if let Some(dentry) = self.special_dir.get(name).cloned() {
                path.dentry = dentry;
                return Ok(path);
            }
        }
        path.run_mount_next();
        match name {
            "" | "." => (),
            ".." => {
                path.run_mount_prev();
                if let Some(dentry) = path.dentry.cache.parent() {
                    path.dentry = dentry;
                }
            }
            s => path.search_child(s).await?,
        }
        Ok(path)
    }
    pub(crate) async fn walk_all(&self, path: (SysR<Arc<VfsFile>>, &str)) -> SysR<Path> {
        let (path, name) = self.walk_path(path).await?;
        self.walk_name(path, name).await
    }
}

pub fn name_invalid(s: &str) -> bool {
    s.bytes().any(|c| match c {
        b'\\' | b'/' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|' => true,
        _ => false,
    })
}

pub fn write_path_to<'a>(src: impl Iterator<Item = &'a str>, dst: &mut [u8]) {
    assert!(dst.len() >= 2);
    let max = dst.len() - 1;
    dst[0] = b'/';
    dst[max] = b'\0';
    let mut p = 0;
    for s in src {
        assert!(p + 1 + s.len() <= max);
        dst[p] = b'/';
        p += 1;
        dst[p..p + s.len()].copy_from_slice(s.as_bytes());
        p += s.len();
    }
    dst[p] = b'\0';
}

pub fn is_absolute_path(s: &str) -> bool {
    match s.as_bytes().first() {
        Some(b'/') | Some(b'\\') => true,
        _ => false,
    }
}
