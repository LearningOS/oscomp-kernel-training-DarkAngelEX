use crate::{
    memory::{
        address::{PageCount, PhyAddr4K, UserAddr4K},
        allocator::frame::FrameAllocator,
        page_table::PTEFlags,
    },
    tools::{error::FrameOOM, range::URange},
};

use super::{PageTable, PageTableEntry};

/// 迭代器当前值总是指向下一次next返回时对应地址
///
struct VaildPteIter<'a> {
    cur: UserAddr4K,
    end: UserAddr4K,
    pt: &'a mut PageTable,
}

impl<'a> VaildPteIter<'a> {
    pub fn new(pt: &'a mut PageTable, r: URange) -> Self {
        Self {
            cur: r.start,
            end: r.end,
            pt,
        }
    }
}

impl<'a> Iterator for VaildPteIter<'a> {
    type Item = (UserAddr4K, &'a mut PageTableEntry);
    fn next(&mut self) -> Option<Self::Item> {
        fn next_pte(a: PhyAddr4K, i: usize) -> &'static mut PageTableEntry {
            &mut a.into_ref().as_pte_array_mut()[i]
        }
        fn base_ceil(a: UserAddr4K, base: usize) -> UserAddr4K {
            let base: usize = 1usize << (12 + 9 * base);
            unsafe { core::intrinsics::assume(a.into_usize() % 0x1000 == 0) };
            unsafe { UserAddr4K::from_usize((a.into_usize() & !(base - 1usize)) + base) }
        }
        if self.cur >= self.end {
            return None;
        }
        stack_trace!();

        let mut cur = self.cur;
        'outer: while cur < self.end {
            let [idx0, idx1, _] = cur.indexes();
            let pte = next_pte(self.pt.root_pa(), idx0);
            if !pte.is_valid() {
                cur = base_ceil(cur, 2);
                continue;
            }
            debug_assert!(pte.is_directory());
            let pte = next_pte(pte.phy_addr(), idx1);
            if !pte.is_valid() {
                cur = base_ceil(cur, 1);
                continue;
            }
            debug_assert!(pte.is_directory());
            // 取出三级页表索引
            let mask = ((1 << 9) - 1) << 12;
            // 加速运行次数最多的内层循环
            let pte = loop {
                let idx2 = (cur.into_usize() & mask) >> 12;
                let pte = next_pte(pte.phy_addr(), idx2);
                if pte.is_valid() {
                    break pte;
                }
                cur.add_page_assign(PageCount(1));
                if cur.into_usize() & mask == 0 {
                    continue 'outer;
                }
                if cur >= self.end {
                    break 'outer;
                }
            };
            debug_assert!(pte.is_leaf());
            debug_assert!(cur < self.end);
            self.cur = cur.add_one_page();
            return Some((cur, pte));
        }
        self.cur = cur;
        None
    }
}

struct EachPteIter<'a> {
    cur: UserAddr4K,
    end: UserAddr4K,
    pt: &'a mut PageTable,
    allocator: &'a mut dyn FrameAllocator,
}

impl<'a> EachPteIter<'a> {
    pub fn new(pt: &'a mut PageTable, r: URange, allocator: &'a mut dyn FrameAllocator) -> Self {
        Self {
            cur: r.start,
            end: r.end,
            pt,
            allocator,
        }
    }
}
impl<'a> Iterator for EachPteIter<'a> {
    type Item = Result<(UserAddr4K, &'a mut PageTableEntry), FrameOOM>;
    /// 返回范围内的每一个页 使用默认帧分配器分配内层
    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.cur;
        if cur >= self.end {
            return None;
        }
        stack_trace!();
        let next_pte = |a: PhyAddr4K, i| &mut a.into_ref().as_pte_array_mut()[i];
        let x = &cur.indexes();
        let pte = next_pte(self.pt.root_pa(), x[0]);
        if !pte.is_valid() {
            if let Err(e) = pte.alloc_by_non_leaf(PTEFlags::V, self.allocator) {
                return Some(Err(e));
            }
        }
        let pte = next_pte(pte.phy_addr(), x[1]);
        if !pte.is_valid() {
            if let Err(e) = pte.alloc_by_non_leaf(PTEFlags::V, self.allocator) {
                return Some(Err(e));
            }
        }
        let pte = next_pte(pte.phy_addr(), x[2]);
        self.cur = cur.add_one_page();
        Some(Ok((cur, pte)))
    }
}

impl PageTable {
    /// 只返回范围内含有 V 标志的页面
    pub fn valid_pte_iter(
        &mut self,
        r: URange,
    ) -> impl Iterator<Item = (UserAddr4K, &mut PageTableEntry)> {
        VaildPteIter::new(self, r)
    }
    /// 返回范围内的每一个 pte 使用默认帧分配器
    pub fn each_pte_iter<'a>(
        &'a mut self,
        r: URange,
        allocator: &'a mut dyn FrameAllocator,
    ) -> impl Iterator<Item = Result<(UserAddr4K, &mut PageTableEntry), FrameOOM>> {
        EachPteIter::new(self, r, allocator)
    }
}
