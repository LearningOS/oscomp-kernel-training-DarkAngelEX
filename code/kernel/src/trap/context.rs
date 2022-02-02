pub struct TrapContext {
    pub x: [usize; 32],   // regs
    pub sstatus: Sstatus, //
    pub sepc: usize,
    pub kernel_satp: usize, // unused
    pub kernel_sp: usize,
    pub trap_handler: usize,
}

pub struct Sstatus {
    bits: usize,
}

impl Sstatus {

}