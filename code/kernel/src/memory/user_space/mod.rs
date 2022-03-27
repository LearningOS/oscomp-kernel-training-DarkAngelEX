use core::mem::MaybeUninit;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use riscv::register::scause::Exception;

use crate::{
    local,
    memory::{
        allocator::frame::{self, iter::SliceFrameDataIter},
        map_segment::handler::{delay::DelayHandler, map_all::MapAllHandler},
        page_table::PTEFlags,
    },
    syscall::SysError,
    tools::{
        container::sync_unsafe_cell::SyncUnsafeCell, error::FrameOOM, range::URange, xasync::TryR,
    },
    user::AutoSum,
};

use self::{
    heap::HeapManager,
    stack::{StackID, StackSpaceManager},
};

use super::{
    address::{OutOfUserRange, PageCount, UserAddr, UserAddr4K},
    allocator::frame::iter::FrameDataIter,
    asid::{self, Asid},
    auxv::AuxHeader,
    map_segment::{handler::AsyncHandler, MapSegment},
    PageTable,
};

pub mod heap;
pub mod stack;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessType {
    pub write: bool,
    pub exec: bool,
    pub user: bool,
}
impl AccessType {
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
        debug_assert!(range.start < range.end);
        Self { range, perm }
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
    map_segment: MapSegment,
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
            stacks: StackSpaceManager::new(),
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
    ) -> TryR<(UserAddr4K, Asid), Box<dyn AsyncHandler>> {
        stack_trace!();
        self.map_segment.page_fault(addr, access)?;
        Ok((addr, self.page_table().asid()))
    }
    async fn a_page_fault(&mut self) {
        todo!()
    }
    /// (stack, user_sp)
    pub fn stack_alloc(
        &mut self,
        stack_reverse: PageCount,
    ) -> Result<(StackID, UserAddr4K), SysError> {
        memory_trace!("UserSpace::stack_alloc");
        let stack = self.stacks.alloc(stack_reverse)?;
        let area_all = stack.area_all();
        let info = stack.info();
        // 绕过 stack 借用检查
        let h = DelayHandler::box_new(area_all.perm);
        self.map_segment.force_push(area_all.range, h)?;
        self.map_segment.force_map(stack.area_init().range)?;
        stack.consume();
        Ok(info)
    }
    pub unsafe fn stack_dealloc(&mut self, stack_id: StackID) {
        memory_trace!("UserSpace::stack_dealloc");
        let user_area = self.stacks.pop_stack_by_id(stack_id);
        self.stacks.dealloc(stack_id);
        self.map_segment.unmap(user_area.range_all());
    }
    pub unsafe fn stack_dealloc_all_except(&mut self, stack_id: StackID) {
        memory_trace!("UserSpace::stack_dealloc_all_except");
        while let Some(user_area) = self.stacks.pop_any_except(stack_id) {
            self.map_segment.unmap(user_area.range_all());
        }
    }
    pub fn heap_resize(&mut self, page_count: PageCount) {
        memory_trace!("UserSpace::heap_resize begin");
        if page_count >= self.heap.size() {
            let map_area = self.heap.set_size_bigger(page_count);
            self.map_segment
                .force_push(map_area.range, DelayHandler::box_new(map_area.perm))
                .unwrap();
        } else {
            let free_area = self.heap.set_size_smaller(page_count);
            self.map_segment.unmap(free_area.range);
        }
        memory_trace!("UserSpace::heap_resize end");
    }
    /// return (space, stack_id, user_sp, entry_point)
    ///
    /// return err if out of memory
    pub fn from_elf(
        elf_data: &[u8],
        stack_reverse: PageCount,
    ) -> Result<(Self, StackID, UserAddr4K, UserAddr), SysError> {
        stack_trace!();
        memory_trace!("UserSpace::from_elf 0");
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
        let mut max_end_4k = unsafe { UserAddr4K::from_usize(0) };
        for i in 0..ph_count {
            let ph = elf.program_header(i).map_err(elf_fail)?;
            if ph.get_type().map_err(elf_fail)? == xmas_elf::program::Type::Load {
                let start_va: UserAddr = (ph.virtual_addr() as usize).into();
                let end_va: UserAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut perm = PTEFlags::U;
                let ph_flags = ph.flags();
                let (mut r, mut w, mut x) = (0, 0, 0);
                if ph_flags.is_read() {
                    perm |= PTEFlags::R;
                    r = 1;
                }
                if ph_flags.is_write() {
                    perm |= PTEFlags::W;
                    w = 1;
                }
                if ph_flags.is_execute() {
                    perm |= PTEFlags::X;
                    x = 1;
                }
                if false {
                    println!("{:?} -> {:?} rwx:{}{}{}", start_va, end_va, r, w, x);
                }
                // assert!(start_va.is_4k_align(), "{:?}", start_va);
                assert!(start_va.floor() >= max_end_4k);
                let map_area = UserArea::new(start_va.floor()..end_va.ceil(), perm);
                max_end_4k = map_area.end();
                let data = &elf.input[ph.offset() as usize - start_va.page_offset()
                    ..(ph.offset() + ph.file_size()) as usize];
                let slice_iter = SliceFrameDataIter::new(data);
                space.force_map_delay_write(map_area, slice_iter)?;
            }
        }
        memory_trace!("UserSpace::from_elf 1");
        // map user stack
        let (stack_id, user_sp) = space.stack_alloc(stack_reverse)?;
        memory_trace!("UserSpace::from_elf 2");
        // set heap
        space.heap_resize(PageCount(1));

        let entry_point = elf.header.pt2.entry_point() as usize;
        Ok((space, stack_id, user_sp, entry_point.into()))
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
    pub unsafe fn clear_user_stack_all(&mut self) {
        while let Some(stack_id) = self.stacks.get_any_id() {
            self.stack_dealloc(stack_id);
        }
    }
    /// return (user_sp, argc, argv, envp)
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
    ///         argv[]...
    ///         envp[]...
    ///     stack bottom
    pub fn push_args(
        &self,
        sp: UserAddr,
        args: &Vec<String>,
        envp: &Vec<String>,
        auxv: &Vec<AuxHeader>,
    ) -> (UserAddr, usize, usize, usize) {
        fn size_of_usize() -> usize {
            core::mem::size_of::<usize>()
        }
        fn get_slice<T>(sp: usize, len: usize) -> &'static mut [T] {
            unsafe { &mut *core::ptr::slice_from_raw_parts_mut(sp as *mut T, len) }
        }
        fn set_zero<T>(sp: usize) {
            unsafe { *(sp as *mut T) = MaybeUninit::zeroed().assume_init() };
        }
        fn xwrite<T>(sp: usize, v: T) {
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
            get_slice(sp, s.len()).copy_from_slice(s.as_bytes());
            set_zero::<u8>(sp + s.len());
        }
        fn write_strings_skip(sp: usize, strs: &Vec<String>) -> (usize, usize) {
            let sp = sp - strs.iter().fold(0, |a, s| a + s.len() + 1);
            (sp, strs.len() + 1)
        }
        fn write_strings(mut sp: usize, strs: &Vec<String>, ptrs: &mut [usize]) {
            debug_assert_eq!(strs.len() + 1, ptrs.len());
            ptrs[strs.len()] = 0;
            for (s, p) in strs.iter().zip(ptrs) {
                *p = sp;
                get_slice(sp, s.len()).copy_from_slice(s.as_bytes());
                sp += s.len();
                set_zero::<u8>(sp);
                sp += 1;
            }
        }
        fn write_auxv(mut sp: usize, auxv: &Vec<AuxHeader>) -> usize {
            let len = auxv.len();
            set_zero::<AuxHeader>(sp);
            let step = 2 * size_of_usize();
            sp -= (len + 1) * step;
            let dst = get_slice(sp, len);
            for (src, dst) in auxv.iter().zip(dst) {
                src.write_to(dst);
            }
            sp
        }

        debug_assert!(self.in_using());

        let _auto_sum = AutoSum::new();

        let mut sp = sp.into_usize();
        sp = usize_align(sp);
        sp -= core::mem::size_of::<usize>();

        let (sp, envp_len) = write_strings_skip(sp, envp);
        let envp_ptr = sp;

        let (sp, args_len) = write_strings_skip(sp, args);
        let args_ptr = sp;

        let mut sp = sp;
        let platform = "RISC-V64";
        sp = write_str_skip(sp, platform);
        sp = align16(sp);
        write_str(sp, platform);

        sp = write_auxv(sp, auxv);

        sp -= envp_len * size_of_usize();
        write_strings(envp_ptr, envp, get_slice(sp, envp_len));
        let envp = sp;

        sp -= args_len * size_of_usize();
        write_strings(args_ptr, args, get_slice(sp, args_len));
        let argv = sp;

        sp -= size_of_usize();
        let argc = args_len - 1;
        xwrite(sp, argc);

        let sp = unsafe { UserAddr::from_usize(sp) };
        (sp, argc, argv, envp)
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
