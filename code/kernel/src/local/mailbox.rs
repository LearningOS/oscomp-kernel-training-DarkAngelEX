use alloc::vec::Vec;

use crate::{
    hart::sfence,
    memory::{
        address::UserAddr4K,
        asid::{Asid, USING_ASID},
    },
    user::NativeAutoSie,
};

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

/// 最多的定向刷表项, 超过这个数升级为全地址空间刷表
const MAX_SPEC_SFENCE: usize = 5;

/// HartMailBox 用来让多核CPU之间安全通信, 目前只用来发送sfence.vma和fence.i指令
pub struct HartMailBox {
    event: MailEvent,
    spec_sfence: Vec<(usize, Option<u16>)>, // VirAddr, ASID
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
                for va_asid in self.spec_sfence.drain(..) {
                    match va_asid {
                        (0, None) => sfence::sfence_vma_all_no_global(),
                        (va, None) => sfence::sfence_vma_va_global(va),
                        (0, Some(asid)) => sfence::sfence_vma_asid(asid as usize),
                        (va, Some(asid)) => sfence::sfence_vma_va_asid(va, asid as usize),
                    }
                }
            }
            self.event.remove(MailEvent::SFENCE_SET);
        }
        debug_assert!(self.event.is_empty(), "{:?}", self.event);
    }
    pub fn set_flag(&mut self, add: MailEvent) {
        self.event |= add;
    }
    pub fn spec_sfence(&mut self, va: Option<UserAddr4K>, asid: Option<Asid>) {
        if self
            .event
            .intersects(MailEvent::SFENCE_VMA_ALL_GLOBAL | MailEvent::SFENCE_VMA_ALL_NO_GLOBAL)
        {
            return;
        }
        if self.spec_sfence.len() > MAX_SPEC_SFENCE {
            self.spec_sfence.clear();
            self.event |= MailEvent::SFENCE_VMA_ALL_NO_GLOBAL;
            return;
        }
        let va = va.map_or(0, |a| a.into_usize());
        let asid = asid.map(|a| a.into_usize() as u16);
        self.spec_sfence.push((va, asid));
        self.event |= MailEvent::SFENCE_SPEC;
    }
}
