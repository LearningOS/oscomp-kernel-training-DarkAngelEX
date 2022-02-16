//! alloc space for kernel stack

use core::{alloc::Layout, ptr::NonNull};

use crate::{
    config::{KERNEL_STACK_SIZE, PAGE_SIZE},
    memory::address::KernelAddr,
    riscv,
    tools::error::HeapOutOfMemory,
};

use super::{
    address::{KernelAddr4K, PageCount},
    allocator::{global_heap_alloc, global_heap_dealloc},
};

pub struct KernelStackTracker {
    ptr: KernelAddr4K,
}

impl Drop for KernelStackTracker {
    fn drop(&mut self) {
        let sp = riscv::current_sp();
        debug_check!(
            sp < self.addr_begin().into_usize() || sp > self.bottom().into_usize(),
            "try free current stack!!!"
        );
        dealloc_kernel_stack(self.ptr);
    }
}
impl KernelStackTracker {
    pub fn bottom(&self) -> KernelAddr4K {
        self.ptr
            .add_page(PageCount::from_usize(KERNEL_STACK_SIZE / PAGE_SIZE))
    }
    pub fn addr_begin(&self) -> KernelAddr4K {
        self.ptr
    }
}

pub fn alloc_kernel_stack() -> Result<KernelStackTracker, HeapOutOfMemory> {
    let layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).unwrap();
    let ptr = global_heap_alloc(layout)?.as_ptr() as usize;
    debug_check!(ptr % PAGE_SIZE == 0);
    Ok(KernelStackTracker {
        ptr: KernelAddr4K::from(KernelAddr::from(ptr)),
    })
}

fn dealloc_kernel_stack(ptr: KernelAddr4K) {
    let layout = Layout::from_size_align(KERNEL_STACK_SIZE, PAGE_SIZE).unwrap();
    global_heap_dealloc(NonNull::new(ptr.into_usize() as *mut u8).unwrap(), layout)
}
