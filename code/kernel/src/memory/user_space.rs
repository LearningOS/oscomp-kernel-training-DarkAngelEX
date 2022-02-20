use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    config::{PAGE_SIZE, USER_MAX_THREADS, USER_STACK_BEGIN, USER_STACK_RESERVE, USER_STACK_SIZE},
    memory::{
        allocator::frame::{self, iter::SliceFrameDataIter},
        page_table::PTEFlags,
    },
    tools::{
        allocator::from_usize_allocator::{FromUsize, UsizeAllocator},
        error::{FrameOutOfMemory, TooManyUserStack},
    },
};

use super::{
    address::{OutOfUserRange, PageCount, UserAddr, UserAddr4K},
    allocator::frame::{
        iter::{FrameDataIter, NullFrameDataIter},
        FrameAllocator,
    },
    asid, PageTable,
};

/// all map to frame.
#[derive(Debug, Copy, Clone)]
pub struct UserArea {
    ubegin: UserAddr4K,
    uend: UserAddr4K,
    perm: PTEFlags,
}

impl UserArea {
    pub fn new(ubegin: UserAddr4K, uend: UserAddr4K, perm: PTEFlags) -> Self {
        Self { ubegin, uend, perm }
    }
    pub fn begin(&self) -> UserAddr4K {
        self.ubegin
    }
    pub fn end(&self) -> UserAddr4K {
        self.uend
    }
    pub fn perm(&self) -> PTEFlags {
        self.perm
    }
    pub fn user_assert(&self) {
        assert!(self.perm & PTEFlags::U != PTEFlags::empty());
    }
}

#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub struct StackID(usize);
impl FromUsize for StackID {
    fn from_usize(v: usize) -> Self {
        Self(v)
    }
}
impl StackID {
    pub fn id(&self) -> usize {
        self.0
    }
}

#[derive(Clone)]
struct StackAllocator {
    allocator: UsizeAllocator,
}

impl Drop for StackAllocator {
    fn drop(&mut self) {
        let using = self.allocator.using();
        assert!(using == 0, "StackAllocator: leak {} stack_id.", using);
    }
}

impl StackAllocator {
    pub const fn new() -> Self {
        Self {
            allocator: UsizeAllocator::new(0),
        }
    }
    pub fn stack_max() -> usize {
        USER_MAX_THREADS
    }
    pub fn alloc(&mut self) -> Result<UsingStack, TooManyUserStack> {
        if self.allocator.using() >= Self::stack_max() {
            return Err(TooManyUserStack);
        }
        let num = self.allocator.alloc();
        let base = USER_STACK_BEGIN;
        let size = USER_STACK_SIZE;
        Ok(UsingStack {
            stack_id: StackID(num),
            stack_begin: UserAddr4K::from_usize_check(base + num * size),
            stack_end: UserAddr4K::from_usize_check(base + (num + 1) * size),
            alloc_num: PageCount::from_usize(USER_STACK_RESERVE / PAGE_SIZE),
        })
    }
    pub unsafe fn dealloc(&mut self, stack_id: usize) {
        self.allocator.dealloc(stack_id)
    }
}

struct UsingStackTracker<'a> {
    allocator: &'a mut StackSpaceManager,
    using_stack: UsingStack,
}
impl<'a> Drop for UsingStackTracker<'a> {
    fn drop(&mut self) {
        unsafe { self.allocator.dealloc(self.using_stack.stack_id()) }
    }
}

impl<'a> UsingStackTracker<'a> {
    pub fn new(allocator: &'a mut StackSpaceManager, using_stack: UsingStack) -> Self {
        Self {
            allocator,
            using_stack,
        }
    }
    pub fn consume(self) -> UsingStack {
        let using_stack = self.using_stack;
        core::mem::forget(self);
        using_stack
    }
    pub fn user_area(&self) -> UserArea {
        let using_stack = &self.using_stack;
        UserArea {
            ubegin: using_stack.stack_end.sub_page(using_stack.alloc_num),
            uend: using_stack.stack_end,
            perm: PTEFlags::U | PTEFlags::R | PTEFlags::W,
        }
    }
    pub fn stack_id(&self) -> StackID {
        self.using_stack.stack_id()
    }
    pub fn bottom_ptr(&self) -> UserAddr4K {
        self.using_stack.stack_end
    }
    /// (stack, user_sp)
    pub fn info(&self) -> (StackID, UserAddr4K) {
        (self.stack_id(), self.bottom_ptr())
    }
}

#[derive(Debug)]
pub enum UserStackCreateError {
    FrameOutOfMemory(FrameOutOfMemory),
    TooManyUserStack(TooManyUserStack),
}
impl From<FrameOutOfMemory> for UserStackCreateError {
    fn from(e: FrameOutOfMemory) -> Self {
        Self::FrameOutOfMemory(e)
    }
}
impl From<TooManyUserStack> for UserStackCreateError {
    fn from(e: TooManyUserStack) -> Self {
        Self::TooManyUserStack(e)
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct UsingStack {
    stack_id: StackID,
    stack_begin: UserAddr4K, // the lower address of stack
    stack_end: UserAddr4K,   // the highest address of stack
    alloc_num: PageCount,    // number of pages allocated
}
impl UsingStack {
    pub fn valid_area(&self) -> UserArea {
        let ubegin = self.stack_end.sub_page(self.alloc_num);
        assert!(self.stack_begin <= ubegin);
        let perm = PTEFlags::U | PTEFlags::R | PTEFlags::W;
        UserArea::new(ubegin, self.stack_end, perm)
    }
    pub fn stack_id(&self) -> StackID {
        self.stack_id
    }
}

#[derive(Clone)]
struct StackSpaceManager {
    allocator: StackAllocator,
    using_stacks: BTreeMap<StackID, UsingStack>,
}

impl Drop for StackSpaceManager {
    fn drop(&mut self) {
        assert!(self.using_stacks.is_empty());
    }
}
impl StackSpaceManager {
    pub const fn new() -> Self {
        Self {
            allocator: StackAllocator::new(),
            using_stacks: BTreeMap::new(),
        }
    }
    pub fn using_size(&self) -> usize {
        self.using_stacks.len()
    }
    pub fn alloc(&mut self) -> Result<UsingStackTracker, TooManyUserStack> {
        let using_stack = self.allocator.alloc()?;
        self.using_stacks
            .insert(using_stack.stack_id(), using_stack)
            .map(|s| panic!("stack double alloc! {:?}", s));
        Ok(UsingStackTracker::new(self, using_stack))
    }
    pub unsafe fn dealloc(&mut self, stack_id: StackID) {
        self.allocator.dealloc(stack_id.id())
    }
    pub fn pop_stack_by_id(&mut self, stack_id: StackID) -> UsingStack {
        self.using_stacks
            .remove(&stack_id)
            .unwrap_or_else(|| panic!("pop_stack_by_id: no find {:?}", stack_id))
    }
    // pub fn iter(&mut self) -> impl Iterator<Item = (&StackID, &UsingStack)> {
    //     self.using_stacks.iter()
    // }
    pub unsafe fn get_any_id(&self) -> Option<StackID> {
        self.using_stacks.first_key_value().map(|(&id, _s)| id)
    }
}

#[derive(Debug, Clone)]
struct HeapManager {
    heap_size: PageCount,
    heap_alloc: PageCount, // lazy alloc cnt
    heap_free: PageCount,  // free count
}
impl Drop for HeapManager {
    fn drop(&mut self) {
        assert!(self.heap_alloc == self.heap_free, "heap leak!");
    }
}
impl HeapManager {
    pub fn new() -> Self {
        Self {
            heap_size: PageCount::from_usize(0),
            heap_alloc: PageCount::from_usize(0),
            heap_free: PageCount::from_usize(0),
        }
    }
    pub fn size(&self) -> PageCount {
        self.heap_size
    }
    pub fn set_size_bigger(&mut self, new: PageCount) {
        assert!(new >= self.heap_size);
        self.heap_size = new;
    }
    pub fn set_size_smaller(&mut self, new: PageCount) -> UserArea {
        let old = self.heap_size;
        assert!(new <= old);
        let perm = PTEFlags::U | PTEFlags::R | PTEFlags::W;
        let ubegin = UserAddr4K::heap_offset(new);
        let uend = UserAddr4K::heap_offset(old);
        let area = UserArea::new(ubegin, uend, perm);
        self.heap_size = new;
        area
    }
    // do this when lazy allocation occurs
    pub fn add_alloc_count(&mut self, n: PageCount) {
        self.heap_alloc += n;
    }
    // do this when resize small
    pub fn add_free_count(&mut self, n: PageCount) {
        self.heap_free += n;
    }
}

/// auto free root space.
///
/// shared between threads, necessary synchronizations operations are required
pub struct UserSpace {
    page_table: PageTable,
    text_area: Vec<UserArea>, // used in drop
    stacks: StackSpaceManager,
    heap: HeapManager,
    // mmap_size: usize,
}

#[derive(Debug)]
pub enum USpaceCreateError {
    FrameOutOfMemory(FrameOutOfMemory),
    ElfAnalysisFail(&'static str),
    TooManyUserStack(TooManyUserStack),
}

impl From<UserStackCreateError> for USpaceCreateError {
    fn from(e: UserStackCreateError) -> Self {
        match e {
            UserStackCreateError::FrameOutOfMemory(e) => Self::FrameOutOfMemory(e),
            UserStackCreateError::TooManyUserStack(e) => Self::TooManyUserStack(e),
        }
    }
}
impl From<FrameOutOfMemory> for USpaceCreateError {
    fn from(e: FrameOutOfMemory) -> Self {
        Self::FrameOutOfMemory(e)
    }
}

impl Drop for UserSpace {
    fn drop(&mut self) {
        stack_trace!();
        memory_trace!("UserSpace::drop begin");
        // free text
        let allocator = &mut frame::defualt_allocator();
        while let Some(area) = self.text_area.pop() {
            self.page_table.unmap_user_range(&area, allocator);
        }
        // free heap
        self.heap_resize(PageCount::from_usize(0), allocator);
        // check stack empty
        assert_eq!(self.stacks.using_size(), 0);
        memory_trace!("UserSpace::drop end");
    }
}

impl UserSpace {
    /// need alloc 4KB to root entry.
    pub fn from_global() -> Result<Self, FrameOutOfMemory> {
        Ok(Self {
            page_table: PageTable::from_global(asid::alloc_asid())?,
            text_area: Vec::new(),
            stacks: StackSpaceManager::new(),
            heap: HeapManager::new(),
        })
    }
    pub fn satp(&self) -> usize {
        self.page_table.satp()
    }
    pub fn using(&mut self) {
        self.page_table.using();
    }
    pub fn map_user_range(
        &mut self,
        map_area: UserArea,
        data_iter: &mut impl FrameDataIter,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOutOfMemory> {
        memory_trace!("UserSpace::map_user_range");
        self.page_table
            .map_user_range(&map_area, data_iter, allocator)?;
        self.text_area.push(map_area);
        Ok(())
    }
    /// (stack, user_sp)
    pub fn stack_alloc(
        &mut self,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(StackID, UserAddr4K), UserStackCreateError> {
        memory_trace!("UserSpace::stack_alloc");
        let stack = self.stacks.alloc().map_err(UserStackCreateError::from)?;
        let user_area = stack.user_area();
        let info = stack.info();
        self.page_table
            .map_user_range(&user_area, &mut NullFrameDataIter, allocator)
            .map_err(UserStackCreateError::from)?;
        stack.consume();
        Ok(info)
    }
    pub unsafe fn stack_dealloc(&mut self, stack_id: StackID, allocator: &mut impl FrameAllocator) {
        memory_trace!("UserSpace::stack_dealloc");
        let user_area = self.stacks.pop_stack_by_id(stack_id);
        self.stacks.dealloc(stack_id);
        self.page_table
            .unmap_user_range(&user_area.valid_area(), allocator);
    }
    pub fn heap_resize(&mut self, page_count: PageCount, allocator: &mut impl FrameAllocator) {
        memory_trace!("UserSpace::heap_resize begin");
        if page_count >= self.heap.size() {
            self.heap.set_size_bigger(page_count);
        } else {
            let free_area = &self.heap.set_size_smaller(page_count);
            let free_count = self.page_table.unmap_user_range_lazy(free_area, allocator);
            self.heap.add_free_count(free_count);
        }
        memory_trace!("UserSpace::heap_resize end");
    }
    /// return (space, stack_id, user_sp, entry_point)
    ///
    /// return err if out of memory
    pub fn from_elf(
        elf_data: &[u8],
        allocator: &mut impl FrameAllocator,
    ) -> Result<(Self, StackID, UserAddr4K, UserAddr), USpaceCreateError> {
        memory_trace!("UserSpace::from_elf 0");
        let elf_fail = USpaceCreateError::ElfAnalysisFail;
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
                if ph_flags.is_read() {
                    perm |= PTEFlags::R;
                }
                if ph_flags.is_write() {
                    perm |= PTEFlags::W;
                }
                if ph_flags.is_execute() {
                    perm |= PTEFlags::X;
                }
                assert!(start_va.is_4k_align());
                assert!(start_va.floor() >= max_end_4k);
                let map_area = UserArea::new(start_va.floor(), end_va.ceil(), perm);
                max_end_4k = map_area.end();
                let data =
                    &elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize];
                let mut slice_iter = SliceFrameDataIter::new(data);
                space.map_user_range(map_area, &mut slice_iter, allocator)?;
            }
        }
        memory_trace!("UserSpace::from_elf 1");
        // map user stack
        let (stack_id, user_sp) = space.stack_alloc(allocator)?;
        memory_trace!("UserSpace::from_elf 2");
        // set heap
        space.heap_resize(PageCount::from_usize(1), allocator);

        let entry_point = elf.header.pt2.entry_point() as usize;
        Ok((space, stack_id, user_sp, entry_point.into()))
    }
    pub fn fork(&mut self, allocator: &mut impl FrameAllocator) -> Result<Self, USpaceCreateError> {
        stack_trace!();
        memory_trace!("UserSpace::fork");
        let page_table = self.page_table.fork(allocator)?;
        let text_area = self.text_area.clone();
        let stacks = self.stacks.clone();
        let heap = self.heap.clone();
        // let oom_fn = USpaceCreateError::from;
        // let stack_fn = USpaceCreateError::from;
        Ok(Self {
            page_table,
            text_area,
            stacks,
            heap,
        })
    }
    pub unsafe fn clear_user_stack_all(&mut self, allocator: &mut impl FrameAllocator) {
        while let Some(stack_id) = self.stacks.get_any_id() {
            self.stack_dealloc(stack_id, allocator);
        }
    }
    pub fn using_size(&self) -> usize {
        self.stacks.using_size()
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
