#![feature(trait_alias)]
#![feature(bench_black_box)]

use std::{fs::File, io::Write, os::unix::prelude::FileExt, sync::Arc, time::Duration};

use async_std::{sync::Mutex, task::block_on};
use clap::{Arg, Command};

extern crate async_std;
extern crate clap;
extern crate fat32;

use fat32::{AsyncRet, BlockDevice};

#[derive(Clone)]
struct BlockFile {
    file: Arc<Mutex<File>>,
}
impl BlockFile {
    pub fn new(file: File) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
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
            let file = self.file.lock().await;
            let offset = (block_id * self.sector_bytes()) as u64;
            file.read_exact_at(buf, offset).unwrap();
            let n = buf.len() / self.sector_bytes();
            println!("driver read  sid: {:>4} n:{}", block_id, n);
            Ok(())
        })
    }
    fn write_block<'a>(&'a self, block_id: usize, buf: &'a [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            assert!(buf.len() % self.sector_bytes() == 0);
            let file = self.file.lock().await;
            let offset = (block_id * self.sector_bytes()) as u64;
            file.write_all_at(buf, offset).unwrap();
            let n = buf.len() / self.sector_bytes();
            println!("driver write sid: {:>4} n:{}", block_id, n);
            Ok(())
        })
    }
}

pub fn ftl_init() {
    fat32::debug_init(|_, _, _| (), || (), || false);
    fat32::console_init(|args| std::io::stdout().write_fmt(args).unwrap());
}

fn main() {
    ftl_init();
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
    println!("!!!!! main exit !!!!!");
}

async fn a_main(path: &str) {
    let file = File::options().read(true).write(true).open(path).unwrap();
    let file = BlockFile::new(file);
    let utc_time = || fat32::UtcTime::base();
    let spawn_fn = |future| {
        async_std::task::spawn(future);
    };
    fat32::xtest::test(file.clone(), utc_time, spawn_fn).await;
    async_std::task::sleep(Duration::from_millis(100)).await;
}
