#![feature(trait_alias)]
#![feature(bench_black_box)]

use std::{
    fs::File, future::Future, io::Write, os::unix::prelude::FileExt, pin::Pin, sync::Arc,
    time::Duration,
};

use async_std::{sync::Mutex, task::block_on};
use clap::{Arg, Command};
use fat32::{ASysR, BlockDevice};
use vfs::{VfsSpawner, ZeroClock};

type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

extern crate async_std;
extern crate clap;
extern crate fat32;
extern crate vfs;

pub const BPB_CID: usize = 10274;

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
    fn read_block<'a>(&'a self, block_id: usize, buf: &'a mut [u8]) -> ASysR<'a, ()> {
        Box::pin(async move {
            assert!(buf.len() % self.sector_bytes() == 0);
            let file = self.file.lock().await;
            let offset = ((block_id + BPB_CID) * self.sector_bytes()) as u64;
            file.read_exact_at(buf, offset).unwrap();
            let n = buf.len() / self.sector_bytes();
            println!("driver read  sid: {:>4} n:{}", block_id, n);
            Ok(())
        })
    }
    fn write_block<'a>(&'a self, block_id: usize, buf: &'a [u8]) -> ASysR<'a, ()> {
        Box::pin(async move {
            assert!(buf.len() % self.sector_bytes() == 0);
            let file = self.file.lock().await;
            let offset = ((block_id + BPB_CID) * self.sector_bytes()) as u64;
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
    let file = Arc::new(BlockFile::new(file));
    fat32::xtest::test(file, Box::new(ZeroClock), Box::new(Spawner)).await;
    async_std::task::sleep(Duration::from_millis(100)).await;
}

/// 用来给文件系统生成同步线程
struct Spawner;

impl VfsSpawner for Spawner {
    fn box_clone(&self) -> Box<dyn VfsSpawner> {
        Box::new(Self)
    }

    fn spawn(&self, future: Async<'static, ()>) {
        async_std::task::spawn(future);
    }
}
