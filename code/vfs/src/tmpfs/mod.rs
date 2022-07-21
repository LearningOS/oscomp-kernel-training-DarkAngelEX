use alloc::boxed::Box;

use crate::{fssp::Fs, inode::FsInode};

pub struct TmpFs {}

impl Fs for TmpFs {
    fn root(&self) -> Box<dyn FsInode> {
        Box::new(TmpFsInode::new())
    }
}

impl TmpFs {
    pub fn new() -> Box<Self> {
        Box::new(Self {})
    }
}

struct TmpFsInode {}

impl FsInode for TmpFsInode {}

impl TmpFsInode {
    fn new() -> Self {
        Self {}
    }
}
