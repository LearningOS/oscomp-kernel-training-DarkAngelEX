use std::{fs::File, os::unix::prelude::FileExt, sync::Mutex};

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{async_tools::ASysR, device::BlockDevice};

const BPB_CID: usize = 0;

/// 唯一的一个对外接口, 打开某个文件并当作磁盘
pub fn get_driver(path: &str) -> Arc<dyn BlockDevice> {
    let file = File::options().read(true).write(true).open(path).unwrap();
    let file = BlockFile::new(file);
    Arc::new(file)
}

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
            let file = self.file.lock().unwrap();
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
            let file = self.file.lock().unwrap();
            let offset = ((block_id + BPB_CID) * self.sector_bytes()) as u64;
            file.write_all_at(buf, offset).unwrap();
            let n = buf.len() / self.sector_bytes();
            println!("driver write sid: {:>4} n:{}", block_id, n);
            Ok(())
        })
    }
}
