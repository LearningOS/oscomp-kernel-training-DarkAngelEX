use riscv::register::sstatus;

#[inline]
pub unsafe fn enable() {
    sstatus::set_sie();
}

pub unsafe fn restore(sie_before: bool) {
    if sie_before {
        enable()
    }
}

pub unsafe fn disable_and_store() -> bool {
    let e = sstatus::read().sie();
    sstatus::clear_sie();
    e
}
