use crate::config::{USER_MMAP_BEGIN, USER_MMAP_END};
use crate::memory::address::{PageCount, UserAddr};
use crate::memory::map_segment::handler::mmap::MmapHandler;
use crate::memory::user_ptr::UserInOutPtr;
use crate::memory::PTEFlags;
use crate::process::fd::Fd;
use crate::syscall::{SysResult, Syscall};
use crate::tools;
use crate::tools::range::URange;
use crate::xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL};

use super::SysError;

const PRINT_SYSCALL_MMAP: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    pub fn sys_mmap(&mut self) -> SysResult {
        stack_trace!();
        let (addr, len, prot, flags, fd, offset): (UserInOutPtr<()>, usize, u32, u32, Fd, usize) =
            self.cx.into();

        if PRINT_SYSCALL_MMAP || true {
            let addr = addr.as_usize();
            println!(
                "sys_mmap addr:{:#x} len:{} prot:{:#x} flags:{:#x} fd:{:?} offset:{}",
                addr, len, prot, flags, fd, offset
            );
        }

        // TODO: other flags
        let prot = MmapProt::from_bits(prot).unwrap();
        let flags = MmapFlags::from_bits(flags).unwrap();
        let page_count = PageCount::page_ceil(len);

        let shared = match (
            flags.contains(MmapFlags::PRIVATE),
            flags.contains(MmapFlags::SHARED),
        ) {
            (true, false) => false,
            (false, true) => true,
            _ => return Err(SysError::EINVAL),
        };

        let mut perm = PTEFlags::empty();
        if prot.contains(MmapProt::WRITE) {
            perm.insert(PTEFlags::W);
        }
        if prot.contains(MmapProt::READ) {
            perm.insert(PTEFlags::R);
        }
        if prot.contains(MmapProt::EXEC) {
            perm.insert(PTEFlags::X);
        }

        let mut alive = self.alive_lock()?;
        let file = if !flags.contains(MmapFlags::ANONYMOUS) {
            let file = alive.fd_table.get(fd).ok_or(SysError::ENFILE)?;
            if !file.can_mmap() {
                return Err(SysError::EBADF);
            }
            Some(file.clone())
        } else {
            None
        };
        let manager = &mut alive.user_space.map_segment;
        let limit = UserAddr::from(USER_MMAP_BEGIN).floor()..UserAddr::from(USER_MMAP_END).ceil();
        let range = match addr.nonnull() {
            Some(ptr) => {
                let start = ptr.as_uptr().ok_or(SysError::EFAULT)?.floor();
                let end = start.add_page(page_count);
                end.valid().map_err(|_| SysError::EFAULT)?;
                tools::range::range_check(&limit, &(start..end)).map_err(|_| SysError::EFAULT)?;
                URange { start, end }
            }
            None => {
                if flags.contains(MmapFlags::FIXED) {
                    return Err(SysError::EFAULT);
                }
                manager
                    .find_free_range(limit, page_count)
                    .ok_or(SysError::ENOMEM)?
            }
        };
        let addr = range.start.into_usize();
        // MmapProt::NONE
        let handler = MmapHandler::box_new(file, offset, perm, shared);
        manager.replace(range, handler)?;
        return Ok(addr);
    }
}

bitflags! {
    pub struct MmapProt: u32 {
        /// Data cannot be accessed
        const NONE = 0;
        /// Data can be read
        const READ = 1 << 0;
        /// Data can be written
        const WRITE = 1 << 1;
        /// Data can be executed
        const EXEC = 1 << 2;
    }
}

bitflags! {
    pub struct MmapFlags: u32 {
        /// Changes are shared.
        const SHARED = 1 << 0;
        /// Changes are private.
        const PRIVATE = 1 << 1;
        /// Place the mapping at the exact address
        const FIXED = 1 << 4;
        /// The mapping is not backed by any file. (non-POSIX)
        const ANONYMOUS = 1 << 5;
    }
}
