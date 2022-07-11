use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::Arc,
};
use ftl_util::error::SysError;

use super::MemFsInode;

pub struct DirInode {
    files: BTreeMap<String, Arc<MemFsInode>>,
}

impl DirInode {
    pub fn new() -> Self {
        DirInode {
            files: BTreeMap::new(),
        }
    }
    pub fn search_any(&self, name: &str) -> Result<Arc<MemFsInode>, SysError> {
        self.files.get(name).cloned().ok_or(SysError::ENOENT)
    }
    pub fn create_dir(&mut self, name: &str, _read_only: bool) -> Result<(), SysError> {
        self.files
            .try_insert(name.to_string(), Arc::new(MemFsInode::new_dir()))
            .map(|_| ())
            .map_err(|_e| SysError::EEXIST)
    }
    pub fn create_file(&mut self, name: &str, _read_only: bool) -> Result<(), SysError> {
        self.files
            .try_insert(name.to_string(), Arc::new(MemFsInode::new_file()))
            .map(|_| ())
            .map_err(|_e| SysError::EEXIST)
    }
    pub fn delete_file(&mut self, name: &str) -> Result<(), SysError> {
        let inode = self.files.get(name).ok_or(SysError::ENOENT)?;
        if !inode.is_file() {
            return Err(SysError::EISDIR);
        }
        match self.files.remove(name) {
            Some(_) => Ok(()),
            None => Err(SysError::ENOENT),
        }
    }
    pub fn delete_dir(&mut self, name: &str) -> Result<(), SysError> {
        let inode = self.files.get(name).ok_or(SysError::ENOENT)?;
        if !inode.is_dir() {
            return Err(SysError::ENOTDIR);
        }
        match self.files.remove(name) {
            Some(_) => Ok(()),
            None => Err(SysError::ENOENT),
        }
    }
    pub fn delete_any(&mut self, name: &str) -> Result<(), SysError> {
        match self.files.remove(name) {
            Some(_) => Ok(()),
            None => Err(SysError::ENOENT),
        }
    }
}
