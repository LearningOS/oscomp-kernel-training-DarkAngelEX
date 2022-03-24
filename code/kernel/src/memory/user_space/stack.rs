use alloc::collections::BTreeMap;

use crate::{
    config::{USER_MAX_THREADS, USER_STACK_BEGIN, USER_STACK_SIZE},
    memory::{
        address::{PageCount, UserAddr4K},
        page_table::PTEFlags,
    },
    syscall::SysError,
    tools::{allocator::from_usize_allocator::FastCloneUsizeAllocator, range::URange},
};

use super::UserArea;

#[derive(Debug, Copy, Clone, PartialOrd, Ord, PartialEq, Eq)]
pub struct StackID(usize);
from_usize_impl!(StackID);

impl StackID {
    pub fn id(&self) -> usize {
        self.0
    }
}

#[derive(Clone)]
struct StackAllocator {
    allocator: FastCloneUsizeAllocator,
}

impl StackAllocator {
    pub const fn new() -> Self {
        Self {
            allocator: FastCloneUsizeAllocator::default(),
        }
    }
    pub fn stack_max() -> usize {
        USER_MAX_THREADS
    }
    pub fn alloc(&mut self, stack_reverse: PageCount) -> Result<UsingStack, SysError> {
        if self.allocator.using() >= Self::stack_max() {
            return Err(SysError::ENOBUFS);
        }
        let num = self.allocator.alloc();
        let base = USER_STACK_BEGIN;
        let size = USER_STACK_SIZE;
        Ok(UsingStack {
            stack_id: StackID(num),
            stack_begin: UserAddr4K::from_usize_check(base + num * size),
            stack_end: UserAddr4K::from_usize_check(base + (num + 1) * size),
            alloc_num: stack_reverse,
        })
    }
    pub unsafe fn dealloc(&mut self, stack_id: usize) {
        self.allocator.dealloc(stack_id)
    }
}

pub struct UsingStackTracker {
    allocator: *mut StackSpaceManager,
    using_stack: UsingStack,
}
impl Drop for UsingStackTracker {
    fn drop(&mut self) {
        unsafe { (*self.allocator).dealloc(self.using_stack.stack_id()) }
    }
}

impl UsingStackTracker {
    pub fn new(allocator: &mut StackSpaceManager, using_stack: UsingStack) -> Self {
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
        UserArea::new(
            using_stack.stack_end.sub_page(using_stack.alloc_num)..using_stack.stack_end,
            PTEFlags::U | PTEFlags::R | PTEFlags::W,
        )
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
        UserArea::new(ubegin..self.stack_end, perm)
    }
    pub fn stack_id(&self) -> StackID {
        self.stack_id
    }
    pub fn range(&self) -> URange {
        URange {
            start: self.stack_begin,
            end: self.stack_end,
        }
    }
}

#[derive(Clone)]
pub struct StackSpaceManager {
    allocator: StackAllocator,
    using_stacks: BTreeMap<StackID, UsingStack>,
}

impl StackSpaceManager {
    pub const fn new() -> Self {
        Self {
            allocator: StackAllocator::new(),
            using_stacks: BTreeMap::new(),
        }
    }
    pub fn alloc(&mut self, stack_reverse: PageCount) -> Result<UsingStackTracker, SysError> {
        let using_stack = self.allocator.alloc(stack_reverse)?;
        if let Some(s) = self
            .using_stacks
            .insert(using_stack.stack_id(), using_stack)
        {
            panic!("stack double alloc! {:?}", s)
        }
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
    pub fn pop_any_except(&mut self, stack_id: StackID) -> Option<UsingStack> {
        let (id, stack) = self.using_stacks.pop_first()?;
        if id != stack_id {
            return Some(stack);
        }
        let ret = self.using_stacks.pop_first().map(|(_id, stack)| stack);
        self.using_stacks.insert(id, stack);
        ret
    }
    // pub fn iter(&mut self) -> impl Iterator<Item = (&StackID, &UsingStack)> {
    //     self.using_stacks.iter()
    // }
    pub unsafe fn get_any_id(&self) -> Option<StackID> {
        self.using_stacks.first_key_value().map(|(&id, _s)| id)
    }
    pub unsafe fn abandon(&mut self) {
        self.using_stacks.clear();
    }
}
