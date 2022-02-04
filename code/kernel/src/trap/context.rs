pub struct TrapContext {
    pub x: [usize; 32],   // regs
    pub sstatus: Sstatus, //
    pub sepc: usize,
    pub kernel_sp: usize,
    pub trap_handler: usize, // unused
}

impl TrapContext {

}

pub struct Sstatus {
    bits: usize,
}

impl Sstatus {

}