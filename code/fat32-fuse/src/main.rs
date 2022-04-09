#![feature(trait_alias)]
use async_std::{
    fs::File,
    io::{prelude::SeekExt, ReadExt, WriteExt},
    sync::Mutex,
    task::block_on,
};
use clap::{Arg, Command};

pub mod xglobal;

extern crate async_std;
extern crate clap;
extern crate fat32;

use fat32::{AsyncRet, BlockDevice};

struct BlockFile {
    file: Mutex<File>,
}
impl BlockFile {
    pub fn new(file: File) -> Self {
        Self {
            file: Mutex::new(file),
        }
    }
}

impl BlockDevice for BlockFile {
    fn sector_bpb(&self) -> usize {
        0
    }
    fn sector_bytes(&self) -> usize {
        512
    }
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            assert!(buf.len() % self.sector_bytes() == 0);
            let mut file = self.file.lock().await;
            file.seek(std::io::SeekFrom::Start(
                (block_id * self.sector_bytes()) as u64,
            ))
            .await
            .map_err(|_e| ())?;
            file.read(buf).await.map_err(|_e| ())?;
            Ok(())
        })
    }

    fn write_block<'a>(&'a self, block_id: usize, buf: &'a [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            assert!(buf.len() % self.sector_bytes() == 0);
            let mut file = self.file.lock().await;
            file.seek(std::io::SeekFrom::Start(
                (block_id * self.sector_bytes()) as u64,
            ))
            .await
            .map_err(|_e| ())?;
            file.write(buf).await.map_err(|_e| ())?;
            Ok(())
        })
    }
}

fn main() {
    let matches = Command::new("fat32 packer")
        .arg(
            Arg::new("source")
                .short('s')
                .long("source")
                .takes_value(true)
                .help("Executable source dir(with backslash)"),
        )
        .get_matches();
    let path = matches.value_of("source").unwrap();
    block_on(a_main(path));
}

async fn a_main(path: &str) {
    let file = File::open(path).await.unwrap();
    let file = BlockFile::new(file);
    fat32::xtest::test(file).await;
}
