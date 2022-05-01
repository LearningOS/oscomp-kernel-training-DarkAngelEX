//! dentry目录项将对应一个inode, 并保证inode和父目录存在
//!

use alloc::{
    boxed::Box,
    collections::LinkedList,
    string::String,
    sync::{Arc, Weak},
};

use crate::{fs::VfsInode, sync::mutex::SpinNoIrqLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NameHash(usize);

pub enum DentryParent {
    None,
    Dentry(Arc<VfsDentry>),
    Mount(!),
}

pub struct VfsDentry {
    name: String,
    hash: usize,
    parent: DentryParent,
    some_children: SpinNoIrqLock<LinkedList<Weak<VfsDentry>>>, // 按LRU排序的不完整的目录下文件 如果此inode为文件或mount将为空
    inode: Weak<VfsInode>,
    mount: Option<()>,
}

impl Drop for VfsDentry {
    fn drop(&mut self) {
        // 使用循环避免Arc递归析构爆栈
        let parent = core::mem::replace(&mut self.parent, DentryParent::None);
        let mut dentry = match parent {
            DentryParent::Dentry(d) => d,
            _ => return,
        };
        loop {
            match Arc::try_unwrap(dentry) {
                Ok(mut d) => {
                    dentry = match core::mem::replace(&mut d.parent, DentryParent::None) {
                        DentryParent::Dentry(d) => d,
                        _ => return,
                    }
                }
                Err(_) => return,
            }
        }
    }
}

fn name_hash(name: &str) -> usize {
    name.as_bytes().iter().copied().fold(0, |n, c| {
        let x = match c {
            c if c.is_ascii_lowercase() => c.to_ascii_uppercase() as usize,
            _ => c as usize,
        };
        n.wrapping_mul(151481979150321234)
            .wrapping_add(x)
            .wrapping_add(1247102954012521241)
    })
}

impl VfsDentry {
    pub fn new_root(inode: Weak<VfsInode>) -> Self {
        Self {
            name: String::new(),
            hash: name_hash(""),
            parent: DentryParent::None,
            some_children: SpinNoIrqLock::new(LinkedList::new()),
            inode,
            mount: None,
        }
    }
}
