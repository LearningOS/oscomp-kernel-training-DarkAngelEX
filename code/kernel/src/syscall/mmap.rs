use crate::config::{PAGE_SIZE, USER_MMAP_RANGE, USER_MMAP_SEARCH_RANGE};
use crate::memory::address::{PageCount, UserAddr};
use crate::memory::allocator::frame;
use crate::memory::map_segment::handler::mmap::MmapHandler;
use crate::memory::user_ptr::UserInOutPtr;
use crate::memory::PTEFlags;
use crate::process::fd::Fd;
use crate::syscall::{SysRet, Syscall};
use crate::{local, tools};

use crate::xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL};

use super::SysError;

const PRINT_SYSCALL_MMAP: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;

impl Syscall<'_> {
    /// prot: 访问权限 R W X
    ///
    /// flags: SHARED | PRIVATE | FIXED | ANONYMOUS
    pub fn sys_mmap(&mut self) -> SysRet {
        stack_trace!();
        let (addr, len, prot, flags, fd, offset): (UserInOutPtr<()>, usize, u32, u32, Fd, usize) =
            self.cx.into();
        const PRINT_THIS: bool = false;
        let len = len.max(PAGE_SIZE);
        // TODO: other flags
        let prot = MmapProt::from_bits(prot).unwrap();
        let flags = MmapFlags::from_bits(flags).unwrap();
        if PRINT_SYSCALL_MMAP || PRINT_THIS {
            let addr = addr.as_usize();
            println!(
                "sys_mmap addr:{:#x} len:{} prot:{:?} flags:{:?} fd:{:?} offset:{} sepc:{:#x}",
                addr, len, prot, flags, fd, offset, self.cx.user_sepc
            );
        }
        let page_count = PageCount::page_ceil(len);
        let shared = match (
            flags.contains(MmapFlags::PRIVATE),
            flags.contains(MmapFlags::SHARED),
        ) {
            (true, false) => false,
            (false, true) => true,
            _ => return Err(SysError::EINVAL),
        };
        let mut alive = self.alive_lock();
        let file = if !flags.contains(MmapFlags::ANONYMOUS) {
            let file = alive.fd_table.get(fd).ok_or(SysError::EBADF)?;
            if !file.can_mmap() {
                println!(
                    "mmap error! {} {} {}",
                    file.type_name(),
                    file.can_read_offset(),
                    file.readable()
                );
                return Err(SysError::EPERM);
            }
            Some(file.clone())
        } else {
            None
        };
        let manager = &mut alive.user_space.map_segment;
        let range = match addr.nonnull() {
            Some(ptr) => {
                let start = ptr.as_uptr().ok_or(SysError::EFAULT)?.floor();
                let end = start.add_page(page_count);
                end.valid().map_err(|_| SysError::EFAULT)?;
                tools::range::range_check(USER_MMAP_RANGE, start..end)
                    .map_err(|_| SysError::EFAULT)?;
                if !flags.contains(MmapFlags::FIXED) {
                    manager
                        .range_is_free(start..end)
                        .map_err(|_| SysError::EFAULT)?;
                }
                start..end
            }
            None => {
                if flags.contains(MmapFlags::FIXED) {
                    return Err(SysError::EFAULT);
                }
                manager
                    .find_free_range(USER_MMAP_SEARCH_RANGE, page_count)
                    .ok_or(SysError::ENOMEM)?
            }
        };
        let addr = range.start;
        let perm = prot.into_perm();

        let handler = MmapHandler::box_new(file, addr, offset, usize::MAX, perm, shared, false);
        manager.replace(range, handler, &mut frame::default_allocator())?;
        let asid = alive.asid();
        drop(alive);
        local::all_hart_sfence_vma_asid(asid);
        if perm.executable() {
            local::all_hart_fence_i();
        }
        let addr = addr.into_usize();
        if PRINT_THIS {
            println!("    -> {:#x}", addr);
        }
        Ok(addr)
    }
    pub fn sys_munmap(&mut self) -> SysRet {
        stack_trace!();
        let (addr, len): (UserInOutPtr<()>, usize) = self.cx.into();
        const PRINT_THIS: bool = false;
        if PRINT_SYSCALL_MMAP || PRINT_THIS {
            let addr = addr.as_usize();
            println!("sys_munmap addr:{:#x} len:{}", addr, len);
        }
        let addr = addr.as_uptr_nullable().ok_or(SysError::EFAULT)?;
        let start = addr.floor();
        let end = UserAddr::try_from((addr.into_usize() + len) as *const u8)?.ceil();
        let mut alive = self.alive_lock();
        let manager = &mut alive.user_space.map_segment;
        manager.unmap(start..end, &mut frame::default_allocator());
        let asid = alive.asid();
        drop(alive);
        local::all_hart_sfence_vma_asid(asid);
        Ok(0)
    }
    pub fn sys_mprotect(&mut self) -> SysRet {
        stack_trace!();
        let (start, len, prot): (UserInOutPtr<()>, usize, u32) = self.cx.into();
        const PRINT_THIS: bool = false;
        if PRINT_SYSCALL_MMAP || PRINT_THIS {
            println!(
                "sys_mprotect start:{:#x} len:{} prot:{:#x}",
                start.as_usize(),
                len,
                prot
            );
        }
        let start = start.as_uptr_nullable().ok_or(SysError::EFAULT)?.floor();
        let end = start.add_page_checked(PageCount::page_ceil(len))?;
        let perm = MmapProt::from_bits_truncate(prot).into_perm();
        let mut alive = self.alive_lock();
        alive.user_space.map_segment.modify_perm(start..end, perm)?;
        let asid = alive.asid();
        drop(alive);
        local::all_hart_sfence_vma_asid(asid);
        if perm.executable() {
            local::all_hart_fence_i();
        }
        Ok(0)
    }
    pub async fn sys_msync(&mut self) -> SysRet {
        stack_trace!();
        let (_start, _len, _flags): (UserInOutPtr<()>, usize, u32) = self.cx.into();
        Ok(0)
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

impl MmapProt {
    pub fn into_perm(self) -> PTEFlags {
        let mut perm = PTEFlags::U;
        if self.contains(MmapProt::READ) {
            perm.insert(PTEFlags::R);
        }
        if self.contains(MmapProt::WRITE) {
            perm.insert(PTEFlags::W);
        }
        if self.contains(MmapProt::EXEC) {
            perm.insert(PTEFlags::X);
        }
        perm
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
