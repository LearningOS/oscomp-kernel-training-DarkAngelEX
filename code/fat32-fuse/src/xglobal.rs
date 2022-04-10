#[no_mangle]
pub extern "C" fn global_console_putchar(c: usize) {
    print!("{}", char::from_u32(c as u32).unwrap());
}
#[no_mangle]
pub extern "C" fn global_console_lock() {}
#[no_mangle]
pub extern "C" fn global_console_unlock() {}

#[no_mangle]
pub extern "C" fn global_xedbug_get_sie() -> u8 {
    0
}

#[no_mangle]
pub extern "C" fn global_xedbug_stack_push(
    msg_ptr: *const u8,
    msg_len: usize,
    file_ptr: *const u8,
    file_len: usize,
    line: u32,
) {
    unsafe {
        let msg =
            core::str::from_utf8_unchecked(&*core::ptr::slice_from_raw_parts(msg_ptr, msg_len));
        let file =
            core::str::from_utf8_unchecked(&*core::ptr::slice_from_raw_parts(file_ptr, file_len));
        STACK.push(StackInfo { msg, file, line })
    }
}
#[no_mangle]
pub extern "C" fn global_xedbug_stack_pop() {
    unsafe { STACK.pop() };
}

struct StackInfo {
    msg: &'static str,
    file: &'static str,
    line: u32,
}

static mut STACK: Vec<StackInfo> = Vec::new();

pub fn show_stack() {
    unsafe {
        for (i, info) in STACK.iter().rev().enumerate() {
            println!("{} {}:{} {}", i, info.file, info.line, info.msg);
        }
    }
}
