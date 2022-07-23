use core::hash::{BuildHasher, BuildHasherDefault, Hasher};

use alloc::sync::Arc;
use ftl_util::sync::{seq_mutex::SeqMutex, Spin};

use crate::dentry::Dentry;

/// 如果要修改名字, 必须使用RCU释放方法保证无锁读取的正确性
pub(crate) struct HashName(SeqMutex<NameInner, Spin>);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct NameHash(pub u64);

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AllHash(pub u64);

struct NameInner {
    all_hash: AllHash,
    name_hash: NameHash,
    parent: usize,
    name: Arc<str>,
}

#[derive(Default)]
struct MyHasher(u64);

impl Hasher for MyHasher {
    fn write(&mut self, bytes: &[u8]) {
        const MUL: u64 = 130923501241292381;
        const ADD: u64 = 423823493280965269;
        self.0 = bytes.iter().copied().fold(self.0, |x, a| {
            x.wrapping_add((a as u64).wrapping_mul(MUL))
                .wrapping_add(ADD)
        });
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

impl HashName {
    pub fn hash_all(base: u64, name: &str) -> AllHash {
        Self::hash_all_by_nh(base, Self::hash_name(name))
    }
    pub fn hash_name(name: &str) -> NameHash {
        NameHash(BuildHasherDefault::<MyHasher>::default().hash_one(name))
    }
    pub fn hash_all_by_nh(base: u64, nh: NameHash) -> AllHash {
        AllHash(base.rotate_left(32).wrapping_add(nh.0))
    }
    pub fn new(parent: *const Dentry, name: &str) -> Self {
        let parent = parent as usize;
        let parnet_hash = parent.rotate_left(32) as u64;
        let name_hash = Self::hash_name(name);
        Self(SeqMutex::new(NameInner {
            all_hash: Self::hash_all_by_nh(parnet_hash, name_hash),
            name_hash,
            parent,
            name: name.into(),
        }))
    }
    /// hash值包含父目录指针
    pub fn all_hash(&self) -> AllHash {
        unsafe { (*self.0.get_ptr()).all_hash }
    }
    pub fn name_hash(&self) -> NameHash {
        unsafe { (*self.0.get_ptr()).name_hash }
    }
    pub fn name(&self) -> Arc<str> {
        unsafe { self.0.unsafe_get().name.clone() }
    }
    /// 此函数不会产生原子开销
    pub fn name_run<T>(&self, mut run: impl FnMut(&str) -> T) -> T {
        self.0.read(|a| run(&a.name))
    }
    /// 父指针和名字都相同
    pub fn all_same(&self, other: &Self) -> bool {
        unsafe {
            if self.0.unsafe_get().all_hash != other.0.unsafe_get().all_hash {
                return false;
            }
        }
        self.0.read(|l| {
            other
                .0
                .read(|r| l.all_hash == r.all_hash && l.parent == r.parent && l.name == r.name)
        })
    }
    /// 序列锁, 只比较字符串
    pub fn name_same(&self, name_hash: NameHash, name: &str) -> bool {
        unsafe {
            if self.0.unsafe_get().name_hash != name_hash {
                return false;
            }
        }
        self.0
            .read(|l| l.name_hash == name_hash && &*l.name == name)
    }
}
