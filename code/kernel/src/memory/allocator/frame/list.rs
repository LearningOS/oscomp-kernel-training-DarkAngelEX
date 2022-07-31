use core::ptr::NonNull;

use crate::{
    config::PAGE_SIZE,
    memory::{address::PhyAddrRef4K, allocator::frame::global::FRAME_OVERWRITE_MAGIC},
    xdebug::{FRAME_DEALLOC_OVERWRITE, FRAME_MODIFY_CHECK, FRAME_RELEASE_CHECK},
};

const FRAME_RELEASE_MAGIC: usize = 0x1232_3895_2892_0389;

const USIZE_PER_FRAME: usize = PAGE_SIZE / core::mem::size_of::<usize>();

/// 这个数据结构用来放置未使用的frame, 同时绕过堆的使用
///
/// Node 从 Frame 中产生, 永不归还
pub struct FrameList {
    head: Option<NonNull<Node>>,
    len: usize,
}

unsafe impl Send for FrameList {}
unsafe impl Sync for FrameList {}

/// 16 字节, 每个 frame 可以产生 4096 / 16 = 256 个 Node
struct Node {
    next: Option<NonNull<Node>>,
    next_copy: Option<NonNull<Node>>,
    release_magic: usize,
    modify_check: [usize; USIZE_PER_FRAME - 3],
}

impl Node {
    pub fn as_ptr(&self) -> NonNull<Self> {
        assert!(core::mem::size_of_val(self) == PAGE_SIZE);
        NonNull::new(self as *const _ as *mut _).unwrap()
    }
    pub fn into_frame(frame: NonNull<Self>) -> PhyAddrRef4K {
        debug_assert!(frame.as_ptr() as usize % PAGE_SIZE == 0);
        unsafe { PhyAddrRef4K::from_usize(frame.as_ptr() as usize) }
    }
    pub fn modify_check(&self) {
        assert_eq!(self.next, self.next_copy);
        if let Some((i, v)) = self
            .modify_check
            .iter()
            .enumerate()
            .find(|(_, &a)| a != FRAME_OVERWRITE_MAGIC)
        {
            let base = self as *const _ as usize;
            let addr = base + i * core::mem::size_of::<usize>();
            panic!("frame has been changed! addr: {:#x} v: {:#x}", addr, v);
        }
    }
}

impl FrameList {
    pub const fn new() -> Self {
        Self { head: None, len: 0 }
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn release_check(frame: PhyAddrRef4K) {
        if FRAME_RELEASE_CHECK {
            let magic = frame.as_mut::<Node>().release_magic;
            assert_ne!(magic, FRAME_RELEASE_MAGIC);
        }
    }
    pub fn push(&mut self, frame: PhyAddrRef4K) {
        debug_assert!(frame.into_usize() % PAGE_SIZE == 0);
        let node: &mut Node = frame.as_mut();
        node.next = self.head;
        self.head = Some(node.as_ptr());
        self.len += 1;
        if FRAME_RELEASE_CHECK {
            assert!(FRAME_DEALLOC_OVERWRITE);
            node.release_magic = FRAME_RELEASE_MAGIC;
        }
        if FRAME_MODIFY_CHECK {
            node.next_copy = node.next;
        }
    }
    pub fn pop(&mut self) -> Option<PhyAddrRef4K> {
        let mut node = self.head?;
        unsafe {
            if FRAME_MODIFY_CHECK {
                // 检测 next 指针是否被修改
                assert_eq!(node.as_mut().next, node.as_mut().next_copy);
            }
            self.head = node.as_mut().next;
            self.len -= 1;
            if FRAME_RELEASE_CHECK {
                if node.as_mut().release_magic != FRAME_RELEASE_MAGIC {
                    panic!(
                        "frame release check fail! ptr: {:#x} value: {:#x}",
                        node.as_ptr() as usize,
                        node.as_mut().release_magic
                    );
                }
                node.as_mut().release_magic = FRAME_OVERWRITE_MAGIC;
            }
            if FRAME_MODIFY_CHECK {
                node.as_mut().modify_check();
            }
            Some(Node::into_frame(node))
        }
    }
}
