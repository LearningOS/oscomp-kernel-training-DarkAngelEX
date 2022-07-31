use core::sync::atomic::AtomicUsize;

/// FTLLocal 将放置在TLS的最上方, 通过tp寄存器直接获取
pub struct FTLCPULocal {
    pub cpuid: AtomicUsize,
}

impl FTLCPULocal {
    pub const fn new(cpuid: usize) -> Self {
        Self {
            cpuid: AtomicUsize::new(cpuid),
        }
    }
    pub fn cpuid(&self) -> usize {
        unsafe { core::ptr::read(self.cpuid.as_mut_ptr()) }
    }
}

#[allow(dead_code)]
static mut STATIC_LOCAL: FTLCPULocal = FTLCPULocal::new(0);

pub fn ftl_local() -> &'static mut FTLCPULocal {
    #[cfg(target_arch = "riscv64")]
    {
        let v = riscv_tp_load();
        unsafe { core::mem::transmute(v) }
    }
    #[cfg(not(target_arch = "riscv64"))]
    {
        unsafe { &mut STATIC_LOCAL }
    }
}

#[cfg(target_arch = "riscv64")]
fn riscv_tp_load() -> usize {
    unsafe {
        let v;
        core::arch::asm!("mv {}, tp", out(reg)v);
        v
    }
}
