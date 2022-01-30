use bitflags::bitflags;

use crate::mm::frame_allocator::frame_dealloc_dpa;

use super::{
    address::{PhyAddr, PhyAddrMasked, PhyAddrRefMasked, VirAddrMasked},
    frame_allocator::{frame_alloc_dpa, FrameTrackerDpa},
};

bitflags! {
    // riscv-privileged 4.3.1 P87
    pub struct PTEFlags: u8 {
        const V = 1 << 0; // valid
        const R = 1 << 1; // readable
        const W = 1 << 2; // writalbe
        const X = 1 << 3; // executable
        const U = 1 << 4; // user mode
        const G = 1 << 5; // global mapping
        const A = 1 << 6; // access, set to 1 after r/w/x
        const D = 1 << 7; // dirty, set to 1 after write
        // RSW 2bit, reserved
    }
}

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct PageTableEntry {
    bits: usize,
}

impl PageTableEntry {
    pub fn new(pa_mask: PhyAddrMasked, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: usize::from(pa_mask) | flags.bits as usize,
        }
    }
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// this function will clear reserved bit in [63:54]
    pub fn phy_addr(&self) -> PhyAddrMasked {
        // (self.bits >> 10 & ((1usize << 44) - 1)).into()
        PhyAddr::from((self.bits & ((1usize << 54) - 1)) << 2).into()
    }
    pub fn flags(&self) -> PTEFlags {
        // PTEFlags::from_bits(self.bits as u8).unwrap()
        unsafe { PTEFlags::from_bits_unchecked(self.bits as u8) }
    }
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
    pub fn is_user(&self) -> bool {
        (self.flags() & PTEFlags::U) != PTEFlags::empty()
    }
    pub fn rsw_8(&self) -> bool {
        (self.bits & 1usize << 8) != 0
    }
    pub fn rsw_9(&self) -> bool {
        (self.bits & 1usize << 9) != 0
    }
    pub fn reserved_bit(&self) -> usize {
        self.bits & ((1usize << 10 - 1) << 54)
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc(&mut self) {
        assert!(self.is_valid());
        frame_dealloc_dpa(self.phy_addr());
        self.bits &= !(PTEFlags::V.bits as usize);
    }
}

pub struct PageTable {
    satp: usize, // [63:60 MODE, 8 is SV39][59:44 ASID][43:0 PPN]
}

// /// Assume that it won't oom when creating/mapping.
impl PageTable {
    // asid set to zero must be success.
    pub fn new(asid: usize) -> Option<Self> {
        let ppn = frame_alloc_dpa()?.consume().ppn();
        Some(PageTable {
            satp: ppn | asid << 44 | 8usize << 60,
        })
    }
    pub fn satp(&self) -> usize {
        self.satp
    }
    pub fn set_asid(&mut self, asid: usize) {
        self.satp = (self.satp & !(0xffff << 44)) | asid << 44
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_satp(satp: usize) -> Self {
        Self { satp }
    }
    fn root_pam(&self) -> PhyAddrMasked {
        PhyAddrMasked::from_satp(self.satp)
    }
    fn find_pte_create(&mut self, vam: VirAddrMasked) -> Result<&mut PageTableEntry, ()> {
        let idxs = vam.indexes();
        let mut parm: PhyAddrRefMasked = self.root_pam().into();
        // auto release when fail
        let mut alloc_0: Option<FrameTrackerDpa> = None;
        let mut alloc_1: Option<FrameTrackerDpa> = None;
        let px = [&mut alloc_0, &mut alloc_1];

        for (i, &idx) in idxs.iter().enumerate() {
            let pte = &mut parm.get_pte_array_mut()[idx];
            if i == 2 {
                core::mem::forget(alloc_0);
                core::mem::forget(alloc_1);
                // alloc_0.map(|a| a.consume());
                // alloc_1.map(|a| a.consume());
                return Ok(pte);
            }
            if !pte.is_valid() {
                let frame = frame_alloc_dpa().ok_or(())?;
                let data = frame.data();
                *px[i] = Some(frame);
                *pte = PageTableEntry::new(data, PTEFlags::V);
            }
            parm = pte.phy_addr().into();
        }
        unreachable!()
    }
    fn find_pte(&self, vam: VirAddrMasked) -> Option<&mut PageTableEntry> {
        let idxs = vam.indexes();
        let mut parm: PhyAddrRefMasked = self.root_pam().into();
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut parm.get_pte_array_mut()[*idx];
            if i == 2 {
                return Some(pte);
            }
            if !pte.is_valid() {
                return None;
            }
            parm = pte.phy_addr().into();
        }
        unreachable!()
    }
    /// clear [vbegin, vend)
    #[allow(unused)]
    pub fn unmap_range(&mut self, vbegin: VirAddrMasked, vend: VirAddrMasked) {
        assert!(vbegin <= vend, "free_range vbegin <= vend");
        if vbegin == vend {
            return;
        }
        let parm: PhyAddrRefMasked = self.root_pam().into();
        let [x0_l, x1_l, x2_l] = vbegin.indexes();
        let [x0_r, x1_r, x2_r] = vend.sub_one_page().indexes();
        Self::free_range_2(parm, x2_l, x2_r, x1_l, x1_r, x0_l, x0_r);
    }
    fn free_range_2(
        mut parm: PhyAddrRefMasked,
        x2_l: usize,
        x2_r: usize,
        x1_l: usize,
        x1_r: usize,
        x0_l: usize,
        x0_r: usize,
    ) {
        for (i, pte) in &mut parm.get_pte_array_mut()[x2_l..x2_r].iter_mut().enumerate() {
            if pte.is_valid() {
                let (mut x1_l, mut x1_r, mut x0_l, mut x0_r) = (x1_l, x1_r, x0_l, x0_r);
                if i != x2_l {
                    (x1_l, x0_l) = (0, 0);
                }
                if i != x2_r {
                    (x1_r, x0_r) = (511, 511);
                }
                Self::free_range_1(pte, x1_l, x1_r, x0_l, x0_r);
                unsafe { pte.dealloc() }
            }
        }
    }
    fn free_range_1(pte: &mut PageTableEntry, x1_l: usize, x1_r: usize, x0_l: usize, x0_r: usize) {
        for (i, pte) in &mut PhyAddrRefMasked::from(pte.phy_addr()).get_pte_array_mut()[x1_l..x1_r]
            .iter_mut()
            .enumerate()
        {
            if pte.is_valid() {
                let (mut x0_l, mut x0_r) = (x0_l, x0_r);
                if i != x1_l {
                    x0_l = 0;
                }
                if i != x1_r {
                    x0_r = 511;
                }
                Self::free_range_0(pte, x0_l, x0_r);
                unsafe { pte.dealloc() }
            }
        }
    }
    fn free_range_0(pte: &mut PageTableEntry, x0_l: usize, x0_r: usize) {
        for pte in &mut PhyAddrRefMasked::from(pte.phy_addr()).get_pte_array_mut()[x0_l..x0_r] {
            if pte.is_valid() {
                unsafe { pte.dealloc() }
            }
        }
    }
    /// if return Err, frame exhausted.
    #[allow(unused)]
    pub fn map(
        &mut self,
        vam: VirAddrMasked,
        parm: PhyAddrRefMasked,
        flags: PTEFlags,
    ) -> Result<(), ()> {
        let pte = self.find_pte_create(vam)?;
        debug_check!(!pte.is_valid(), "vam {:?} is mapped before mapping", vam);
        *pte = PageTableEntry::new(parm.into(), flags | PTEFlags::V);
        Ok(())
    }
    #[allow(unused)]
    pub fn unmap(&mut self, vam: VirAddrMasked) {
        let pte = self.find_pte(vam).expect("unmap invalid virtual address!");
        assert!(pte.is_valid(), "pte {:?} is invalid before unmapping", pte);
        *pte = PageTableEntry::empty();
    }
    pub fn translate(&self, vam: VirAddrMasked) -> Option<PageTableEntry> {
        self.find_pte(vam).map(|pte| *pte)
    }
}

// pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
//     let page_table = PageTable::from_token(token);
//     let mut start = ptr as usize;
//     let end = start + len;
//     let mut v = Vec::new();
//     while start < end {
//         let start_va = VirAddr::from(start);
//         let mut vpn = start_va.floor();
//         let ppn = page_table.translate(vpn).unwrap().ppn();
//         vpn.step();
//         let mut end_va: VirAddr = vpn.into();
//         end_va = end_va.min(VirAddr::from(end));
//         if end_va.page_offset() == 0 {
//             v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
//         } else {
//             v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
//         }
//         start = end_va.into();
//     }
//     v
// }
