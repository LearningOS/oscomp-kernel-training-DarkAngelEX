#[macro_use]
pub mod stack;

static mut CURRENT_SIE: Option<fn() -> bool> = None;

pub fn sie_init(current_sie: fn() -> bool) {
    unsafe {
        CURRENT_SIE.replace(current_sie);
    }
}

pub fn assert_sie_closed() {
    stack_trace!();
    unsafe {
        if let Some(f) = CURRENT_SIE {
            assert!(!f());
        }
    }
}
