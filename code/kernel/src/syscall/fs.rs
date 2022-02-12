use crate::{riscv::sbi, trap::context::TrapContext, user};

const FD_STDIN: usize = 0;
const FD_STDOUT: usize = 1;

pub fn sys_write(trap_context: &mut TrapContext, args: [usize; 3]) -> isize {
    memory_trace!("sys_write entry");
    // fd: usize, buf: *const u8, len: usize;
    let (fd, buf, len) = (args[0], args[1] as *const u8, args[2]);
    // println!("sys_write call fd = {} buf: {:#x} len: {}", args[0], args[1], args[2]);

    let buf = match user::translated_user_read_range(trap_context, buf, len) {
        Ok(buf) => buf,
        Err(e) => {
            println!("[FTL OS]sys_write invalid ptr: {:?}", e);
            return -1;
        }
    };
    let src = buf.access();
    let str = match core::str::from_utf8(&*src) {
        Ok(str) => str,
        Err(e) => {
            println!("[FTL OS]sys_write utf8 error: {}", e);
            return -1;
        }
    };
    match fd {
        FD_STDOUT => {
            print!("{}", str);
            return str.len() as isize;
        }
        _ => todo!("[FTL OS]sys_write unsupported fd"),
    };
}

pub fn sys_read(trap_context: &mut TrapContext, args: [usize; 3]) -> isize {
    memory_trace!("sys_read entry");
    let (fd, buf, len) = (args[0], args[1] as *mut u8, args[2]);

    let buf = match user::translated_user_write_range(trap_context, buf, len) {
        Ok(buf) => buf,
        Err(e) => {
            println!("[FTL OS]sys_read invalid ptr: {:?}", e);
            return -1;
        }
    };

    assert_eq!(len, 1, "Only support len = 1 in sys_read!");

    match fd {
        FD_STDIN => {
            let mut c: usize;
            loop {
                c = sbi::console_getchar();
                if c == 0 {
                    // suspend_current_and_run_next();
                    continue;
                } else {
                    break;
                }
            }
            let ch = c as u8;
            // println!("read from sbi: <{}>", ch as char);
            buf.access_mut()[0] = ch;
            1
        }
        _ => {
            panic!("Unsupported fd in sys_read!");
        }
    }
}
