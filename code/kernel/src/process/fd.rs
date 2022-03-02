use alloc::{collections::BTreeMap, sync::Arc};

use crate::{
    fs::{File, Stdin, Stdout},
    tools::{
        allocator::from_usize_allocator::{FromUsize, FromUsizeAllocator},
        ForwardWrapper,
    },
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Fd(usize);

impl FromUsize for Fd {
    fn from_usize(num: usize) -> Self {
        Fd(num)
    }
}
impl Fd {
    pub fn new(x: usize) -> Self {
        Self(x)
    }
    pub fn into_usize(&self) -> usize {
        self.0
    }
    pub fn assert_eq(&self, x: usize) {
        assert_eq!(self.0, x)
    }
}

pub type FdAllocator = FromUsizeAllocator<Fd, ForwardWrapper>;

pub struct FdTable {
    map: BTreeMap<Fd, Arc<dyn File>>,
    fd_allocator: FdAllocator,
}

impl FdTable {
    pub fn new() -> Self {
        let mut ret = Self {
            map: BTreeMap::new(),
            fd_allocator: FdAllocator::new(0),
        };
        ret.insert(Arc::new(Stdin)).assert_eq(0);
        ret.insert(Arc::new(Stdout)).assert_eq(1);
        ret.insert(Arc::new(Stdout)).assert_eq(2);
        ret
    }
    pub fn insert(&mut self, v: Arc<dyn File>) -> Fd {
        let fd = self.fd_allocator.alloc();
        self.map
            .try_insert(fd, v)
            .map_err(|_| ())
            .expect("FdTable double insert same fd");
        fd
    }
    pub fn get(&self, fd: Fd) -> Option<&Arc<dyn File>> {
        self.map.get(&fd)
    }
    pub fn remove(&mut self, fd: Fd) -> Option<Arc<dyn File>> {
        self.map.remove(&fd)
    }
}
