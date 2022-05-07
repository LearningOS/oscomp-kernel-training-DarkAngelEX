use crate::{
    config::{
        DIRECT_MAP_BEGIN, DIRECT_MAP_END, DIRECT_MAP_OFFSET, KERNEL_OFFSET_FROM_DIRECT_MAP,
        KERNEL_TEXT_BEGIN, KERNEL_TEXT_END,
    },
    memory::{
        address::{PageCount, PhyAddr, PhyAddr4K, PhyAddrRef, VirAddr},
        allocator::frame,
    },
    sync::SleepMutex,
};

use super::BlockDevice;
use alloc::boxed::Box;
use fat32::AsyncRet;
use virtio_drivers::{VirtIOBlk, VirtIOHeader};

#[allow(unused)]
const VIRTIO0: usize = 0x10001000;

pub struct VirtIOBlock(SleepMutex<VirtIOBlk<'static>>);

impl BlockDevice for VirtIOBlock {
    fn sector_bpb(&self) -> usize {
        0
    }
    fn sector_bytes(&self) -> usize {
        512
    }
    fn read_block<'a>(&'a self, mut block_id: usize, buf: &'a mut [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            stack_trace!();
            let io = &mut *self.0.lock().await;
            for buf in buf.chunks_mut(self.sector_bytes()) {
                io.read_block(block_id, buf)
                    .expect("Error when reading VirtIOBlk");
                block_id += 1;
            }
            Ok(())
        })
    }
    fn write_block<'a>(&'a self, mut block_id: usize, buf: &'a [u8]) -> AsyncRet<'a> {
        Box::pin(async move {
            stack_trace!();
            let io = &mut *self.0.lock().await;
            for buf in buf.chunks(self.sector_bytes()) {
                io.write_block(block_id, buf)
                    .expect("Error when reading VirtIOBlk");
                block_id += 1;
            }
            Ok(())
        })
    }
}

impl VirtIOBlock {
    #[allow(unused)]
    pub fn new() -> Self {
        unsafe {
            Self(SleepMutex::new(
                VirtIOBlk::new(&mut *((VIRTIO0 + DIRECT_MAP_OFFSET) as *mut VirtIOHeader)).unwrap(),
            ))
        }
    }
}

#[no_mangle]
pub extern "C" fn virtio_dma_alloc(pages: PageCount) -> PhyAddr4K {
    frame::global::alloc_successive(pages).unwrap().into()
}

#[no_mangle]
pub extern "C" fn virtio_dma_dealloc(mut pa: PhyAddr4K, pages: usize) -> i32 {
    for _ in 0..pages {
        unsafe { frame::global::dealloc(pa.into()) };
        pa = pa.add_one_page();
    }
    0
}

#[no_mangle]
pub extern "C" fn virtio_phys_to_virt(paddr: PhyAddr) -> PhyAddrRef {
    paddr.into()
}

#[no_mangle]
pub extern "C" fn virtio_virt_to_phys(vaddr: VirAddr) -> PhyAddr {
    //println!("v:{:x}",vaddr);
    let vaddr = vaddr.into_usize();
    let pa = match vaddr {
        DIRECT_MAP_BEGIN..=DIRECT_MAP_END => vaddr - DIRECT_MAP_OFFSET,
        KERNEL_TEXT_BEGIN..=KERNEL_TEXT_END => {
            vaddr - KERNEL_OFFSET_FROM_DIRECT_MAP - DIRECT_MAP_OFFSET
        }
        _ => panic!("virtio_virt_to_phys error: {:#x}", vaddr),
    };
    PhyAddr::from_usize(pa)
}
