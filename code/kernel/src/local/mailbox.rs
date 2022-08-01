use alloc::{boxed::Box, vec::Vec};

use crate::{hart::sfence, memory::asid::USING_ASID, user::NativeAutoSie};

bitflags! {
    pub struct MailEvent: usize {
        const FENCE_I                   = 1 << 0;
        const SFENCE_VMA_ALL_GLOBAL     = 1 << 1;
        const SFENCE_VMA_ALL_NO_GLOBAL  = 1 << 2;
        const SFENCE_SPEC               = 1 << 3;
    }
}

impl MailEvent {
    pub const SFENCE_SET: Self = Self::SFENCE_VMA_ALL_GLOBAL
        .union(Self::SFENCE_VMA_ALL_NO_GLOBAL)
        .union(Self::SFENCE_SPEC);
}

/// HartMailBox 用来让多核CPU之间安全通信, 目前只用来发送sfence.vma和fence.i指令
pub struct HartMailBox {
    event: MailEvent,
    spec_sfence: Vec<Box<dyn FnOnce()>>,
}

impl HartMailBox {
    pub const fn new() -> Self {
        Self {
            event: MailEvent::empty(),
            spec_sfence: Vec::new(),
        }
    }
    // swap_nonoverlapping 比 swap 更快
    pub fn swap(&mut self, other: &mut Self) {
        unsafe { core::ptr::swap_nonoverlapping(self, other, 1) }
    }
    pub fn is_empty(&self) -> bool {
        self.event.is_empty()
    }
    pub fn handle(&mut self) {
        if self.is_empty() {
            return;
        }
        if self.event.intersects(MailEvent::FENCE_I) {
            sfence::fence_i();
            self.event.remove(MailEvent::FENCE_I);
        }
        if self.event.intersects(MailEvent::SFENCE_SET) {
            let _sie = NativeAutoSie::new(); // 关中断
            if self.event.contains(MailEvent::SFENCE_VMA_ALL_GLOBAL) {
                sfence::sfence_vma_all_global();
                self.spec_sfence.clear();
            } else if self.event.contains(MailEvent::SFENCE_VMA_ALL_NO_GLOBAL) {
                assert!(!USING_ASID);
                sfence::sfence_vma_all_no_global();
                self.spec_sfence.clear();
            } else if self.event.contains(MailEvent::SFENCE_SPEC) {
                self.spec_sfence.drain(..).for_each(|f| f());
            }
            self.event.remove(MailEvent::SFENCE_SET);
        }
        debug_assert!(self.event.is_empty(), "{:?}", self.event);
    }
    pub fn set_flag(&mut self, add: MailEvent) {
        self.event |= add;
    }
    pub fn spec_sfence(&mut self, f: impl FnOnce() + 'static) {
        if self
            .event
            .intersects(MailEvent::SFENCE_VMA_ALL_GLOBAL | MailEvent::SFENCE_VMA_ALL_NO_GLOBAL)
        {
            return;
        }
        self.spec_sfence.push(Box::new(f));
        self.event |= MailEvent::SFENCE_SPEC;
    }
}
