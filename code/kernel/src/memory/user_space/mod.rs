use core::mem::MaybeUninit;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use riscv::register::scause::Exception;

use crate::{
    config::{PAGE_SIZE, USER_KRW_RANDOM_RANGE, USER_KRX_RANGE, USER_STACK_RESERVE},
    local,
    memory::{
        allocator::frame::{self, iter::SliceFrameDataIter},
        auxv::AT_PHDR,
        map_segment::handler::{delay::DelayHandler, map_all::MapAllHandler},
        page_table::PTEFlags,
    },
    syscall::SysError,
    timer,
    tools::{
        self, container::sync_unsafe_cell::SyncUnsafeCell, error::FrameOOM, range::URange,
        xasync::TryR, DynDropRun,
    },
    user::AutoSum,
    xdebug::CLOSE_RANDOM,
};

use self::{heap::HeapManager, stack::StackSpaceManager};

use super::{
    address::{OutOfUserRange, PageCount, UserAddr, UserAddr4K},
    allocator::frame::iter::FrameDataIter,
    asid::{self, Asid},
    auxv::AuxHeader,
    map_segment::{handler::AsyncHandler, MapSegment},
    PageTable,
};

core::arch::global_asm!(include_str!("./kload.S"));

pub mod heap;
pub mod stack;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessType {
    pub write: bool,
    pub exec: bool,
    pub user: bool,
}
impl AccessType {
    pub const RO: Self = Self {
        write: false,
        exec: false,
        user: true,
    };
    pub const RW: Self = Self {
        write: true,
        exec: false,
        user: true,
    };
    pub fn from_exception(e: Exception) -> Result<Self, ()> {
        match e {
            Exception::LoadPageFault => Ok(Self {
                write: false,
                exec: false,
                user: true,
            }),
            Exception::InstructionPageFault => Ok(Self {
                write: false,
                exec: true,
                user: true,
            }),
            Exception::StorePageFault => Ok(Self {
                write: true,
                exec: false,
                user: true,
            }),
            _ => Err(()),
        }
    }
    pub fn check(self, flag: PTEFlags) -> Result<(), ()> {
        ((!self.write || flag.contains(PTEFlags::R))
            && (!self.exec || flag.contains(PTEFlags::X))
            && (!self.user || flag.contains(PTEFlags::U)))
        .then_some(())
        .ok_or(())
    }
}

/// all map to frame.
#[derive(Debug, Clone)]
pub struct UserArea {
    range: URange,
    perm: PTEFlags,
}

impl UserArea {
    pub fn new(range: URange, perm: PTEFlags) -> Self {
        debug_assert!(
            range.start < range.end,
            "{:#x}..{:#x}",
            range.start.into_usize(),
            range.end.into_usize()
        );
        Self { range, perm }
    }
    pub fn new_urw(range: URange) -> Self {
        Self::new(range, PTEFlags::U | PTEFlags::R | PTEFlags::W)
    }
    pub fn begin(&self) -> UserAddr4K {
        self.range.start
    }
    pub fn end(&self) -> UserAddr4K {
        self.range.end
    }
    pub fn perm(&self) -> PTEFlags {
        self.perm
    }
    pub fn user_assert(&self) {
        assert!(self.perm & PTEFlags::U != PTEFlags::empty());
    }
}

/// auto free root space.
///
/// shared between threads, necessary synchronizations operations are required
pub struct UserSpace {
    pub map_segment: MapSegment,
    stacks: StackSpaceManager,
    heap: HeapManager,
    // mmap_size: usize,
}

unsafe impl Send for UserSpace {}
unsafe impl Sync for UserSpace {}

impl Drop for UserSpace {
    fn drop(&mut self) {
        stack_trace!();
        self.map_segment.clear();
        let allocator = &mut frame::defualt_allocator();
        self.page_table_mut().free_user_directory_all(allocator);
    }
}

impl UserSpace {
    /// need alloc 4KB to root entry.
    pub fn from_global() -> Result<Self, FrameOOM> {
        let pt = Arc::new(SyncUnsafeCell::new(PageTable::from_global(
            asid::alloc_asid(),
        )?));
        Ok(Self {
            map_segment: MapSegment::new(pt),
            stacks: StackSpaceManager::new(PageCount::page_floor(USER_STACK_RESERVE)),
            heap: HeapManager::new(),
        })
    }
    fn page_table(&self) -> &PageTable {
        unsafe { &*self.map_segment.page_table.get() }
    }
    pub fn page_table_arc(&self) -> Arc<SyncUnsafeCell<PageTable>> {
        self.map_segment.page_table.clone()
    }
    pub(super) fn page_table_mut(&mut self) -> &mut PageTable {
        unsafe { &mut *self.map_segment.page_table.get() }
    }
    pub fn asid(&self) -> Asid {
        self.page_table().asid()
    }
    pub unsafe fn using(&self) {
        local::task_local().page_table = self.page_table_arc();
        self.raw_using();
    }
    pub unsafe fn raw_using(&self) {
        self.page_table().using();
    }
    pub fn in_using(&self) -> bool {
        self.page_table().in_using()
    }
    fn force_map_delay(&mut self, map_area: UserArea) -> Result<(), SysError> {
        stack_trace!();
        self.map_segment
            .force_push(map_area.range, MapAllHandler::box_new(map_area.perm))
    }
    fn force_map_delay_write(
        &mut self,
        map_area: UserArea,
        data: impl FrameDataIter,
    ) -> Result<(), SysError> {
        stack_trace!();
        let r = map_area.range;
        self.map_segment
            .force_push(r.clone(), MapAllHandler::box_new(map_area.perm))?;
        stack_trace!();
        self.map_segment.force_write_range(r, data)
    }
    pub fn page_fault(
        &mut self,
        addr: UserAddr4K,
        access: AccessType,
    ) -> TryR<DynDropRun<(UserAddr4K, Asid)>, Box<dyn AsyncHandler>> {
        stack_trace!();
        self.map_segment.page_fault(addr, access)
    }
    async fn a_page_fault(&mut self) {
        todo!()
    }
    /// (stack, user_sp)
    pub fn stack_init(&mut self, stack_reverse: PageCount) -> Result<UserAddr4K, SysError> {
        stack_trace!();
        // 绕过 stack 借用检查
        let h = DelayHandler::box_new(PTEFlags::R | PTEFlags::W | PTEFlags::U);
        self.map_segment.force_push(self.stacks.max_area(), h)?;
        self.map_segment
            .force_map(self.stacks.init_area(stack_reverse))?;
        Ok(self.stacks.init_sp())
    }
    pub fn get_brk(&self) -> UserAddr<u8> {
        self.heap.brk()
    }
    pub fn reset_brk(&mut self, new_brk: UserAddr<u8>) -> Result<(), SysError> {
        let ms = &mut self.map_segment;
        self.heap.set_brk(new_brk, move |r, f| {
            if f {
                ms.force_push(r.range, DelayHandler::box_new(r.perm))
            } else {
                Ok(ms.unmap(r.range))
            }
        })?;
        Ok(())
    }
    pub fn heap_init(&mut self, base: UserAddr4K, init_size: PageCount) {
        let map_area = self.heap.init(base, init_size);
        self.map_segment
            .force_push(map_area.range, DelayHandler::box_new(map_area.perm))
            .unwrap();
    }
    pub fn heap_resize(&mut self, _page_count: PageCount) {
        stack_trace!();
        todo!()
        // if page_count >= self.heap.size() {
        //     let map_area = self.heap.set_size_bigger(page_count);
        //     self.map_segment
        //         .force_push(map_area.range, DelayHandler::box_new(map_area.perm))
        //         .unwrap();
        // } else {
        //     let free_area = self.heap.set_size_smaller(page_count);
        //     self.map_segment.unmap(free_area.range);
        // }
    }
    /// return (space, stack_id, user_sp, entry_point)
    ///
    /// return err if out of memory
    pub fn from_elf(
        elf_data: &[u8],
        stack_reverse: PageCount,
    ) -> Result<(Self, UserAddr4K, UserAddr<u8>, Vec<AuxHeader>), SysError> {
        stack_trace!();
        let elf_fail = |str| {
            println!("elf analysis error: {}", str);
            SysError::EFAULT
        };
        let mut space = Self::from_global()?;

        let elf = xmas_elf::ElfFile::new(elf_data).map_err(elf_fail)?;
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();

        let mut head_va = 0;
        let mut max_end_4k = unsafe { UserAddr4K::from_usize(0) };

        for i in 0..ph_count {
            stack_trace!();
            let ph = elf.program_header(i).map_err(elf_fail)?;
            if ph.get_type().map_err(elf_fail)? == xmas_elf::program::Type::Load {
                let start_va: UserAddr<u8> = (ph.virtual_addr() as usize).into();
                let end_va: UserAddr<u8> = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                if start_va.is_4k_align() {
                    head_va = start_va.into_usize();
                }
                let mut perm = PTEFlags::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    perm |= PTEFlags::R;
                }
                if ph_flags.is_write() {
                    perm |= PTEFlags::W;
                }
                if ph_flags.is_execute() {
                    perm |= PTEFlags::X;
                }
                if false {
                    println!(
                        "\t{:?} -> {:?} \tperm: {:?} file_size:{:#x}",
                        start_va,
                        end_va,
                        perm,
                        ph.file_size()
                    );
                }
                // assert!(start_va.is_4k_align(), "{:?}", start_va);
                assert!(start_va.floor() >= max_end_4k);
                let map_area = UserArea::new(start_va.floor()..end_va.ceil(), perm);
                max_end_4k = map_area.end();
                stack_trace!();
                // 用一个小trick补全偏移量
                let data = &elf.input[ph.offset() as usize - start_va.page_offset()
                    ..(ph.offset() + ph.file_size()) as usize];
                stack_trace!();
                let slice_iter = SliceFrameDataIter::new(data);
                space.force_map_delay_write(map_area, slice_iter)?;
            }
        }
        stack_trace!();
        let entry_point = elf_header.pt2.entry_point() as usize;
        if false {
            println!("\tentry_point: {:#x}", entry_point);
        }
        let mut auxv = AuxHeader::generate(
            elf_header.pt2.ph_entry_size() as usize,
            ph_count as usize,
            entry_point,
        );
        // Get ph_head addr for auxv
        let ph_head_addr = head_va + elf.header.pt2.ph_offset() as usize;
        auxv.push(AuxHeader {
            aux_type: AT_PHDR,
            value: ph_head_addr as usize,
        });
        stack_trace!();
        // map kernel_load
        struct KRXFrameIter;
        impl FrameDataIter for KRXFrameIter {
            fn len(&self) -> usize {
                PAGE_SIZE
            }
            fn write_to(&mut self, dst: &mut [u8; 4096]) -> Result<(), ()> {
                extern "C" {
                    fn __kload_begin();
                    fn __kload_end();
                }
                let begin = __kload_begin as *const u8;
                let end = __kload_end as *const u8;
                unsafe {
                    let src = core::slice::from_ptr_range(begin..end);
                    debug_assert!(src.len() < PAGE_SIZE);
                    dst[..src.len()].copy_from_slice(src);
                    dst[src.len()..].fill(0);
                }
                Ok(())
            }
        }
        space.force_map_delay_write(
            UserArea::new(USER_KRX_RANGE, PTEFlags::R | PTEFlags::X | PTEFlags::U),
            KRXFrameIter,
        )?;
        struct KRWRandomIter;
        impl FrameDataIter for KRWRandomIter {
            fn len(&self) -> usize {
                PAGE_SIZE
            }
            fn write_to(&mut self, dst: &mut [u8; 4096]) -> Result<(), ()> {
                let seed = match CLOSE_RANDOM {
                    true => 1,
                    false => timer::get_time_ticks().into_usize() as u64 ^ 0xcdba,
                };
                let mut s = (0x1u64, seed);
                for dst in dst {
                    *dst = s.0.wrapping_add(s.1) as u8;
                    s = tools::xor_shift_128_plus(s);
                }
                Ok(())
            }
        }
        space.force_map_delay_write(
            UserArea::new(
                USER_KRW_RANDOM_RANGE,
                PTEFlags::R | PTEFlags::W | PTEFlags::U,
            ),
            KRWRandomIter,
        )?;

        // map user stack:
        let user_sp = space.stack_init(stack_reverse)?;
        stack_trace!();
        // set heap
        space.heap_init(max_end_4k, PageCount(1));

        Ok((space, user_sp, entry_point.into(), auxv))
    }
    pub fn fork(&mut self) -> Result<Self, SysError> {
        memory_trace!("UserSpace::fork");
        // let page_table = self.page_table_mut().fork(allocator)?;
        let map_segment = self.map_segment.fork()?;
        let stacks = self.stacks.clone();
        let heap = self.heap.clone();
        let ret = Self {
            map_segment,
            stacks,
            heap,
        };
        Ok(ret)
    }
    /// return (user_sp, argc, argv, envp)
    ///
    /// from https://www.cnblogs.com/likaiming/p/11193697.html
    ///
    /// sp  ->  argc
    ///         argv[0]
    ///         argv[1]
    ///         ...
    ///         argv[n] = NULL
    ///
    ///         envp[0]
    ///         envp[1]
    ///         ...
    ///         envp[n] = NULL
    ///
    ///         auxv[0]
    ///         auxv[1]
    ///         ...
    ///         auxv[n] = NULL
    ///
    ///         (padding 16 bytes)
    ///
    ///         rand bytes (16 bytes)
    ///
    ///         String identifying platform = "RISC-V64"
    ///
    ///         (random stack offset for safety)
    ///
    ///         argv[]...
    ///         envp[]...
    ///     stack bottom
    ///
    pub fn push_args(
        &self,
        sp: UserAddr4K,
        args: &[String],
        envp: &[String],
        auxv: &[AuxHeader],
        reverse: PageCount,
    ) -> (UserAddr<u8>, usize, usize, usize) {
        fn size_of_usize() -> usize {
            core::mem::size_of::<usize>()
        }
        fn get_slice<T>(sp: usize, len: usize) -> &'static mut [T] {
            unsafe { &mut *core::ptr::slice_from_raw_parts_mut(sp as *mut T, len) }
        }
        fn set_zero<T>(sp: usize, _r: &[T]) {
            unsafe { *(sp as *mut T) = MaybeUninit::zeroed().assume_init() };
        }
        fn write_v<T>(sp: usize, v: T) {
            unsafe { (sp as *mut T).write(v) };
        }
        fn usize_align(sp: usize) -> usize {
            sp & -(size_of_usize() as isize) as usize
        }
        fn align16(sp: usize) -> usize {
            sp & -16isize as usize
        }
        fn write_str_skip(sp: usize, s: &str) -> usize {
            sp - (s.len() + 1)
        }
        fn write_str(sp: usize, s: &str) {
            let bytes = s.as_bytes();
            get_slice(sp, s.len()).copy_from_slice(bytes);
            set_zero(sp + s.len(), bytes);
        }
        /// -> (sp, strs.len() + 1)
        fn write_strings_skip(sp: usize, strs: &[String]) -> (usize, usize) {
            let sp = sp - strs.iter().fold(0, |a, s| a + s.len() + 1);
            (sp, strs.len() + 1)
        }
        fn write_strings(mut sp: usize, strs: &[String], ptrs: &mut [usize]) {
            debug_assert_eq!(strs.len() + 1, ptrs.len());
            ptrs[strs.len()] = 0;
            for (s, p) in strs.iter().zip(ptrs) {
                *p = sp;
                let bytes = s.as_bytes();
                get_slice(sp, s.len()).copy_from_slice(bytes);
                sp += s.len();
                set_zero(sp, bytes);
                sp += 1;
            }
        }
        fn write_auxv_skip(sp: usize, auxv: &[AuxHeader]) -> usize {
            sp - (auxv.len() + 1) * 2 * size_of_usize()
        }
        fn write_auxv(mut sp: usize, auxv: &[AuxHeader]) {
            let dst = get_slice(sp, auxv.len());
            auxv.iter()
                .zip(dst)
                .for_each(|(src, dst)| src.write_to(dst));
            sp += auxv.len() * core::mem::size_of::<AuxHeader>();
            set_zero(sp, auxv);
        }

        debug_assert!(self.in_using());

        let sp_top = sp;
        let mut sp = sp.into_usize();
        sp = usize_align(sp);
        sp -= core::mem::size_of::<usize>();

        let (sp, envp_len) = write_strings_skip(sp, envp);
        let envp_ptr = sp;

        let (sp, args_len) = write_strings_skip(sp, args);
        let args_ptr = sp;

        let rand = 0;
        let sp = sp - rand % PAGE_SIZE;

        let mut sp = sp;
        let platform = "RISC-V64";
        sp = write_str_skip(sp, platform);
        sp = align16(sp);
        let plat_ptr = sp;

        sp = write_auxv_skip(sp, auxv);
        let auxv_ptr = sp;

        sp -= envp_len * size_of_usize();
        let r_envp = sp;

        sp -= args_len * size_of_usize();
        let r_argv = sp;

        sp -= size_of_usize();
        let argc_ptr = sp;

        let sp = unsafe { UserAddr::from_usize(sp) };

        debug_assert!(sp.ceil() <= sp_top);
        debug_assert!(sp_top.sub_page(reverse) <= sp.floor());

        // 直接在用户虚拟地址上操作

        if false {
            println!("args: {:#x} len: {}", r_argv, args_len);
            println!("envp: {:#x} len: {}", r_envp, envp_len);
            println!("auxv: {:#x} len: {}", auxv_ptr, auxv.len() + 1);
        }

        let _auto_sum = AutoSum::new();
        write_str(plat_ptr, platform);
        write_auxv(auxv_ptr, auxv);
        write_strings(envp_ptr, envp, get_slice(r_envp, envp_len));
        write_strings(args_ptr, args, get_slice(r_argv, args_len));
        write_v(argc_ptr, args_len - 1);

        (sp, args_len - 1, r_argv, r_envp)
    }
    pub fn push_args_size(args: &[String], envp: &[String]) -> PageCount {
        fn xsum<T>(strs: &[T], mut f: impl FnMut(&T) -> usize) -> usize {
            strs.iter().fold(0, move |x, v| x + f(v))
        }
        let mut size = 0;
        size += 8;
        size += (args.len() + 1) * core::mem::size_of::<usize>();
        size += (envp.len() + 1) * core::mem::size_of::<usize>();
        size += AuxHeader::reverse();
        size += 16 * 2;
        size += "RISC-V64".len() + 1;
        size += 16;
        size += PAGE_SIZE; // (random_stack)
        size += xsum(args, |s| s.len() + 1);
        size += xsum(envp, |s| s.len() + 1);
        PageCount::page_ceil(size)
    }
}

#[derive(Debug)]
pub enum UserPtrTranslateErr {
    OutOfUserRange(OutOfUserRange),
}
impl From<OutOfUserRange> for UserPtrTranslateErr {
    fn from(e: OutOfUserRange) -> Self {
        Self::OutOfUserRange(e)
    }
}
