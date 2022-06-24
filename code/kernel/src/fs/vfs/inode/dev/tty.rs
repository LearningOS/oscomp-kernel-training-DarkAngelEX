use crate::fs::{AsyncFile, File};

pub struct TtyInode;

impl File for TtyInode {
    fn readable(&self) -> bool {
        todo!()
    }
    fn writable(&self) -> bool {
        todo!()
    }
    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> AsyncFile {
        todo!()
    }
    fn write<'a>(&'a self, read_only: &'a [u8]) -> AsyncFile {
        todo!()
    }
}
