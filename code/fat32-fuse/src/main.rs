#![feature(trait_alias)]

use async_std::{
    fs::File,
    io::{prelude::SeekExt, ReadExt},
    sync::Mutex,
    task::block_on,
};

pub mod xglobal;

extern crate async_std;
extern crate fat32;

use fat32::{AsyncRet, LogicBlockDevice, BLOCK_SZ};

struct BlockFile {
    file: Mutex<File>,
}

impl LogicBlockDevice for BlockFile {
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            assert_eq!(buf.len(), BLOCK_SZ);
            self.file
                .lock()
                .await
                .seek(std::io::SeekFrom::Start((block_id * BLOCK_SZ) as u64));
            todo!()
        })
    }

    fn write_block(&self, block_id: usize, buf: &[u8]) -> AsyncRet {
        assert_eq!(buf.len(), BLOCK_SZ);
        todo!()
    }
}

fn main() {
    // let file = File::fr
    block_on(fat32::test::test());
}
