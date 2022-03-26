#![allow(dead_code)]

use crate::{
    config::PAGE_SIZE,
    memory::{
        address::{PageCount, PhyAddr4K, PhyAddrRef4K, UserAddr4K, VirAddr4K},
        allocator::frame::FrameAllocator,
        page_table::{FrameDataIter, PTEFlags, PageTable, PageTableEntry, UserArea},
    },
    syscall::SysError,
    tools::error::FrameOOM,
    xdebug::PRINT_MAP_ALL,
};

impl PageTable {
    #[inline(always)]
    fn next_lr<'a, 'b, const N1: usize, const N: usize>(
        l: &'a [usize; N1],
        r: &'b [usize; N1],
        xbegin: &'a [usize; N],
        xend: &'b [usize; N],
        i: usize,
    ) -> (&'a [usize; N], &'b [usize; N], bool) {
        let xl = if i == 0 {
            l.rsplit_array_ref::<N>().1
        } else {
            xbegin
        };
        let xr = if i == r[0] - l[0] {
            r.rsplit_array_ref::<N>().1
        } else {
            xend
        };
        (xl, xr, xl.eq(xbegin) && xr.eq(xend))
    }
    #[inline(always)]
    fn indexes_diff<const N: usize>(begin: &[usize; N], end: &[usize; N]) -> PageCount {
        fn get_num<const N: usize>(a: &[usize; N]) -> usize {
            let mut value = 0;
            for &x in a {
                value <<= 9;
                value += x;
            }
            value
        }
        let x0 = get_num(begin);
        let x1 = get_num(end) + 1;
        PageCount(x1 - x0)
    }

    pub fn force_map_user(
        &mut self,
        addr: UserAddr4K,
        pte_fn: impl FnOnce() -> Result<PageTableEntry, SysError>,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), SysError> {
        let pte = self.get_pte_user(addr, allocator)?;
        assert!(!pte.is_valid(), "remap of {:?}", addr);
        *pte = pte_fn()?;
        Ok(())
    }
    pub fn force_unmap_user(&mut self, addr: UserAddr4K, pte_fn: impl FnOnce(PageTableEntry)) {
        let next_pte = |a: PhyAddr4K, i| &mut a.into_ref().as_pte_array_mut()[i];
        let x = &addr.indexes();
        let pte = next_pte(self.root_pa(), x[0]);
        assert!(pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[1]);
        assert!(pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[2]);
        assert!(pte.is_leaf());
        pte_fn(*pte);
        *pte = PageTableEntry::empty();
    }
    /// 页必须已经被映射
    pub fn force_convert_user<T>(
        &mut self,
        addr: UserAddr4K,
        pte_fn: impl FnOnce(&mut PageTableEntry) -> T,
    ) -> T {
        let next_pte = |a: PhyAddr4K, i| &mut a.into_ref().as_pte_array_mut()[i];
        let x = &addr.indexes();
        let pte = next_pte(self.root_pa(), x[0]);
        assert!(pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[1]);
        assert!(pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[2]);
        assert!(pte.is_leaf());
        pte_fn(pte)
    }
    /// 不处理未映射的页
    pub fn lazy_convert_user(
        &mut self,
        addr: UserAddr4K,
        pte_fn: impl FnOnce(&mut PageTableEntry),
    ) {
        macro_rules! return_or_check {
            ($pte: ident, $a: expr) => {
                if !$pte.is_valid() {
                    return;
                }
                debug_assert!($a);
            };
        }
        let next_pte = |a: PhyAddr4K, i| &mut a.into_ref().as_pte_array_mut()[i];
        let x = &addr.indexes();
        let pte = next_pte(self.root_pa(), x[0]);
        return_or_check!(pte, pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[1]);
        return_or_check!(pte, pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[2]);
        return_or_check!(pte, pte.is_leaf());
        pte_fn(pte);
    }
    /// 返回的 pte 一定包含 V 标志位
    pub fn try_get_pte_user(&mut self, addr: UserAddr4K) -> Option<&mut PageTableEntry> {
        macro_rules! return_or_check {
            ($pte: ident, $a: expr) => {
                if !$pte.is_valid() {
                    return None;
                }
                debug_assert!($a);
            };
        }
        let next_pte = |a: PhyAddr4K, i| &mut a.into_ref().as_pte_array_mut()[i];
        let x = &addr.indexes();
        let pte = next_pte(self.root_pa(), x[0]);
        return_or_check!(pte, pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[1]);
        return_or_check!(pte, pte.is_directory());
        let pte = next_pte(pte.phy_addr(), x[2]);
        return_or_check!(pte, pte.is_leaf());
        unsafe { Some(&mut *(pte as *mut _)) }
    }
    pub fn get_pte_user(
        &mut self,
        addr: UserAddr4K,
        allocator: &mut impl FrameAllocator,
    ) -> Result<&mut PageTableEntry, FrameOOM> {
        let next_pte = |a: PhyAddr4K, i| &mut a.into_ref().as_pte_array_mut()[i];
        stack_trace!();
        let x = &addr.indexes();
        let pte = next_pte(self.root_pa(), x[0]);
        if !pte.is_valid() {
            pte.alloc_by(PTEFlags::V, allocator)?;
        }
        let pte = next_pte(pte.phy_addr(), x[1]);
        if !pte.is_valid() {
            pte.alloc_by(PTEFlags::V, allocator)?;
        }
        let pte = next_pte(pte.phy_addr(), x[2]);
        Ok(pte)
    }
    /// if range have been map will panic.
    ///
    /// return Err if out of memory
    pub fn map_user_range(
        &mut self,
        map_area: &UserArea,
        data_iter: &mut impl FrameDataIter,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOOM> {
        memory_trace!("PageTable::map_user_range");
        assert!(map_area.perm().contains(PTEFlags::U));
        let ubegin = map_area.begin();
        let uend = map_area.end();
        if ubegin == uend {
            return Ok(());
        }
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let flags = map_area.perm();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        return match map_user_range_0(ptes, l, r, flags, data_iter, allocator, ubegin) {
            Ok(ua) => {
                debug_assert_eq!(ua, uend);
                Ok(())
            }
            Err(ua) => {
                // realease page table
                let alloc_area = UserArea::new(ubegin..ua, flags);
                self.unmap_user_range(&alloc_area, allocator);
                Err(FrameOOM)
            }
        };

        /// return value:
        ///
        /// Ok: next ua
        ///
        /// Err: err ua, There is no space assigned to this location
        #[inline(always)]
        fn map_user_range_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            flags: PTEFlags,
            data_iter: &mut impl FrameDataIter,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> Result<UserAddr4K, UserAddr4K> {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                memory_trace!("PageTable::map_user_range_0-0");
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if !pte.is_valid() {
                    pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                }
                let ptes = PageTable::ptes_from_pte(pte);
                ua = map_user_range_1(ptes, l, r, flags, data_iter, allocator, ua)?;
            }
            Ok(ua)
        }
        #[inline(always)]
        fn map_user_range_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            flags: PTEFlags,
            data_iter: &mut impl FrameDataIter,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> Result<UserAddr4K, UserAddr4K> {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                memory_trace!("PageTable::map_user_range_1-0");
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if !pte.is_valid() {
                    pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                }
                let ptes = PageTable::ptes_from_pte(pte);
                ua = map_user_range_2(ptes, l, r, flags, data_iter, allocator, ua)?;
            }
            Ok(ua)
        }
        #[inline(always)]
        fn map_user_range_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            flags: PTEFlags,
            data_iter: &mut impl FrameDataIter,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> Result<UserAddr4K, UserAddr4K> {
            for pte in &mut ptes[l[0]..=r[0]] {
                assert!(!pte.is_valid(), "remap of {:?}", ua);
                memory_trace!("PageTable::map_user_range_2-0");
                let par = allocator.alloc().map_err(|_| ua)?.consume();
                memory_trace!("PageTable::map_user_range_2-1");
                // fill zero if return Error
                let _ = data_iter.write_to(par.as_bytes_array_mut());
                memory_trace!("PageTable::map_user_range_2-2");
                *pte = PageTableEntry::new(par.into(), flags | PTEFlags::V);
                ua = ua.add_one_page();
            }
            Ok(ua)
        }
    }

    pub fn unmap_user_range(&mut self, map_area: &UserArea, allocator: &mut impl FrameAllocator) {
        assert!(map_area.perm().contains(PTEFlags::U));
        let ubegin = map_area.begin();
        let uend = map_area.end();
        if ubegin == uend {
            return;
        }
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let ua = unmap_user_range_0(ptes, l, r, allocator, ubegin);
        debug_assert_eq!(ua, uend);
        return;

        #[inline(always)]
        fn unmap_user_range_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> UserAddr4K {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                let ptes = PageTable::ptes_from_pte(pte);
                ua = unmap_user_range_1(ptes, l, r, allocator, ua);
                if full {
                    unsafe { pte.dealloc_by(allocator) };
                }
            }
            ua
        }
        #[inline(always)]
        fn unmap_user_range_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> UserAddr4K {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                let ptes = PageTable::ptes_from_pte(pte);
                ua = unmap_user_range_2(ptes, l, r, allocator, ua);
                if full {
                    unsafe { pte.dealloc_by(allocator) };
                }
            }
            ua
        }
        #[inline(always)]
        fn unmap_user_range_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> UserAddr4K {
            for pte in &mut ptes[l[0]..=r[0]].iter_mut() {
                assert!(pte.is_leaf(), "unmap invalid leaf: {:?}", ua);
                unsafe { pte.dealloc_by(allocator) };
                ua = ua.add_one_page();
            }
            ua
        }
    }

    /// lazy copy all range, skip invalid leaf.
    pub fn copy_user_range_lazy(
        dst: &mut Self,
        src: &mut Self,
        map_area: &UserArea,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOOM> {
        memory_trace!("copy_user_range_lazy");
        map_area.user_assert();
        let ubegin = map_area.begin();
        let uend = map_area.end();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let src_ptes = src.root_pa().into_ref().as_pte_array_mut();
        let dst_ptes = dst.root_pa().into_ref().as_pte_array_mut();
        return match copy_user_range_lazy_0(dst_ptes, src_ptes, l, r, ubegin, allocator) {
            Ok(ua) => {
                debug_assert_eq!(ua, uend);
                Ok(())
            }
            Err(ua) => {
                let alloc_area = UserArea::new(ubegin..ua, PTEFlags::U);
                dst.unmap_user_range_lazy(alloc_area, allocator);
                Err(FrameOOM)
            }
        };
        #[inline(always)]
        fn copy_user_range_lazy_0(
            dst_ptes: &mut [PageTableEntry; 512],
            src_ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<UserAddr4K, UserAddr4K> {
            memory_trace!("copy_user_range_lazy_0");
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            let dst_it = dst_ptes[l[0]..=r[0]].iter_mut();
            let src_it = src_ptes[l[0]..=r[0]].iter_mut();
            for (i, (dst_pte, src_pte)) in &mut dst_it.zip(src_it).enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if src_pte.is_valid() {
                    assert!(src_pte.is_directory());
                    memory_trace!("copy_user_range_lazy_0 0");
                    dst_pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                    memory_trace!("copy_user_range_lazy_0 1");
                    let dst_ptes = PageTable::ptes_from_pte(dst_pte);
                    let src_ptes = PageTable::ptes_from_pte(src_pte);
                    ua = copy_user_range_lazy_1(dst_ptes, src_ptes, l, r, ua, allocator)?;
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            Ok(ua)
        }
        #[inline(always)]
        fn copy_user_range_lazy_1(
            dst_ptes: &mut [PageTableEntry; 512],
            src_ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<UserAddr4K, UserAddr4K> {
            memory_trace!("copy_user_range_lazy_1");
            // println!("lazy_1 ua: {:#x}", ua.into_usize());
            let xbegin = &[0];
            let xend = &[511];
            let dst_it = dst_ptes[l[0]..=r[0]].iter_mut();
            let src_it = src_ptes[l[0]..=r[0]].iter_mut();
            for (i, (dst_pte, src_pte)) in &mut dst_it.zip(src_it).enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if src_pte.is_valid() {
                    assert!(src_pte.is_directory());
                    dst_pte.alloc_by(PTEFlags::V, allocator).map_err(|_| ua)?;
                    let dst_ptes = PageTable::ptes_from_pte(dst_pte);
                    let src_ptes = PageTable::ptes_from_pte(src_pte);
                    ua = copy_user_range_lazy_2(dst_ptes, src_ptes, l, r, ua, allocator)?;
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            Ok(ua)
        }
        #[inline(always)]
        fn copy_user_range_lazy_2(
            dst_ptes: &mut [PageTableEntry; 512],
            src_ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<UserAddr4K, UserAddr4K> {
            memory_trace!("copy_user_range_lazy_2");
            let dst_it = dst_ptes[l[0]..=r[0]].iter_mut();
            let src_it = src_ptes[l[0]..=r[0]].iter_mut();
            for (dst_pte, src_pte) in &mut dst_it.zip(src_it) {
                if src_pte.is_valid() {
                    assert!(src_pte.is_leaf() && src_pte.is_user());
                    let perm =
                        src_pte.flags() & (PTEFlags::U | PTEFlags::R | PTEFlags::W | PTEFlags::X);
                    dst_pte.alloc_by(perm, allocator).map_err(|_| ua)?;
                    let src = src_pte.phy_addr().into_ref().as_usize_array();
                    let dst = dst_pte.phy_addr().into_ref().as_usize_array_mut();
                    dst[0..512].copy_from_slice(&src[0..512]);
                    memory_trace!("copy_user_range_lazy_2");
                }
                ua = ua.add_one_page();
            }
            memory_trace!("copy_user_range_lazy_2");
            Ok(ua)
        }
    }
    pub fn unmap_user_range_lazy(
        &mut self,
        map_area: UserArea,
        allocator: &mut impl FrameAllocator,
    ) -> PageCount {
        stack_trace!();
        assert!(map_area.perm().contains(PTEFlags::U));
        let ubegin = map_area.begin();
        let uend = map_area.end();
        let page_count = PageCount::from_usize(0);
        if ubegin == uend {
            return page_count;
        }
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let (page_count, ua) = unmap_user_range_lazy_0(ptes, l, r, page_count, allocator, ubegin);
        debug_assert_eq!(ua, uend);
        return page_count;

        #[inline(always)]
        fn unmap_user_range_lazy_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            mut page_count: PageCount,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> (PageCount, UserAddr4K) {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                    let ptes = PageTable::ptes_from_pte(pte);
                    (page_count, ua) =
                        unmap_user_range_lazy_1(ptes, l, r, page_count, allocator, ua);
                    if full {
                        unsafe { pte.dealloc_by(allocator) };
                    }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            (page_count, ua)
        }
        #[inline(always)]
        fn unmap_user_range_lazy_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            mut page_count: PageCount,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> (PageCount, UserAddr4K) {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    assert!(pte.is_directory(), "unmap invalid directory: {:?}", ua);
                    let ptes = PageTable::ptes_from_pte(pte);
                    (page_count, ua) =
                        unmap_user_range_lazy_2(ptes, l, r, page_count, allocator, ua);
                    if full {
                        unsafe { pte.dealloc_by(allocator) };
                    }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r));
                }
            }
            (page_count, ua)
        }
        #[inline(always)]
        fn unmap_user_range_lazy_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            mut page_count: PageCount,
            allocator: &mut impl FrameAllocator,
            mut ua: UserAddr4K,
        ) -> (PageCount, UserAddr4K) {
            for pte in &mut ptes[l[0]..=r[0]].iter_mut() {
                if pte.is_valid() {
                    assert!(pte.is_leaf(), "unmap invalid leaf: {:?}", ua);
                    unsafe { pte.dealloc_by(allocator) };
                    page_count += PageCount(0);
                }
                ua = ua.add_one_page();
            }
            (page_count, ua)
        }
    }
    pub fn map_direct_range(
        &mut self,
        vbegin: VirAddr4K,
        pbegin: PhyAddrRef4K,
        size: usize,
        flags: PTEFlags,
        allocator: &mut impl FrameAllocator,
    ) -> Result<(), FrameOOM> {
        if size == 0 {
            return Ok(());
        }
        assert!(size % PAGE_SIZE == 0);
        let par = self.root_pa().into_ref();
        let vend = unsafe { VirAddr4K::from_usize(usize::from(vbegin) + size) };
        let l = &vbegin.indexes();
        let r = &vend.sub_one_page().indexes();
        if PRINT_MAP_ALL {
            println!(
                "map_range: {:#x} - {:#x} size = {}",
                usize::from(vbegin),
                usize::from(vend),
                size
            );
            println!("l:{:?}", l);
            println!("r:{:?}", r);
        }
        let ptes = par.as_pte_array_mut();
        // clear 12 + 9 * 3 = 39 bit
        let va = map_direct_range_0(ptes, l, r, flags, vbegin, pbegin.into(), allocator)?;
        debug_assert_eq!(va, vend);
        return Ok(());

        #[inline(always)]
        fn map_direct_range_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            flags: PTEFlags,
            mut va: VirAddr4K,
            mut pa: PhyAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<VirAddr4K, FrameOOM> {
            // println!("level 0: {:?} {:?}-{:?}", va, l, r);
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    // 1GB page table
                    assert!(!pte.is_valid(), "1GB pagetable: remap");
                    debug_assert!(va.into_usize() % (PAGE_SIZE * (1 << (9 * 2))) == 0);
                    // if true || PRINT_MAP_ALL {
                    //     println!("map 1GB {:?} -> {:?}", va, pa);
                    // }
                    *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                    unsafe {
                        va = VirAddr4K::from_usize(va.into_usize() + PAGE_SIZE * (1 << (9 * 2)));
                        pa = PhyAddr4K::from_usize(pa.into_usize() + PAGE_SIZE * (1 << (9 * 2)));
                    }
                } else {
                    if !pte.is_valid() {
                        pte.alloc_by(PTEFlags::V, allocator)?;
                    }
                    let ptes = PageTable::ptes_from_pte(pte);
                    (va, pa) = map_direct_range_1(ptes, l, r, flags, va, pa, allocator)?
                }
            }
            Ok(va)
        }
        #[inline(always)]
        fn map_direct_range_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            flags: PTEFlags,
            mut va: VirAddr4K,
            mut pa: PhyAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> Result<(VirAddr4K, PhyAddr4K), FrameOOM> {
            // println!("level 1: {:?} {:?}-{:?}", va, l, r);
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    // 1MB page table
                    assert!(!pte.is_valid(), "1MB pagetable: remap");
                    debug_assert!(va.into_usize() % (PAGE_SIZE * (1 << 9)) == 0);
                    *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                    unsafe {
                        va = VirAddr4K::from_usize(va.into_usize() + PAGE_SIZE * (1 << 9));
                        pa = PhyAddr4K::from_usize(pa.into_usize() + PAGE_SIZE * (1 << 9));
                    }
                } else {
                    if !pte.is_valid() {
                        pte.alloc_by(PTEFlags::V, allocator)?;
                    }
                    let ptes = PageTable::ptes_from_pte(pte);
                    (va, pa) = map_direct_range_2(ptes, l, r, flags, va, pa, allocator);
                }
            }
            Ok((va, pa))
        }
        #[inline(always)]
        fn map_direct_range_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            flags: PTEFlags,
            mut va: VirAddr4K,
            mut pa: PhyAddr4K,
            _allocator: &mut impl FrameAllocator,
        ) -> (VirAddr4K, PhyAddr4K) {
            // println!("level 2: {:?} {:?}-{:?}", va, l, r);
            for pte in &mut ptes[l[0]..=r[0]] {
                assert!(!pte.is_valid(), "remap of {:?} -> {:?}", va, pa);
                // if true || PRINT_MAP_ALL {
                //     println!("map: {:?} -> {:?}", va, pa);
                // }
                *pte = PageTableEntry::new(pa, flags | PTEFlags::V);
                unsafe {
                    va = VirAddr4K::from_usize(va.into_usize() + PAGE_SIZE);
                    pa = PhyAddr4K::from_usize(pa.into_usize() + PAGE_SIZE);
                }
            }
            (va, pa)
        }
    }

    /// clear [vbegin, vend)
    pub fn unmap_direct_range(&mut self, vbegin: VirAddr4K, vend: VirAddr4K) {
        assert!(vbegin <= vend, "free_range vbegin <= vend");
        if vbegin == vend {
            return;
        }
        let l = &vbegin.indexes();
        let r = &vend.sub_one_page().indexes();
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        return unmap_direct_range_0(ptes, l, r);

        #[inline(always)]
        fn unmap_direct_range_0(ptes: &mut [PageTableEntry; 512], l: &[usize; 3], r: &[usize; 3]) {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    // 1GB page table
                    debug_assert!(pte.is_leaf());
                    *pte = PageTableEntry::empty();
                } else {
                    debug_assert!(pte.is_directory());
                    let ptes = PageTable::ptes_from_pte(pte);
                    unmap_direct_range_1(ptes, l, r);
                }
            }
        }
        #[inline(always)]
        fn unmap_direct_range_1(ptes: &mut [PageTableEntry; 512], l: &[usize; 2], r: &[usize; 2]) {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if full {
                    debug_assert!(pte.is_leaf());
                    *pte = PageTableEntry::empty();
                } else {
                    debug_assert!(pte.is_directory());
                    let ptes = PageTable::ptes_from_pte(pte);
                    unmap_direct_range_2(ptes, l, r);
                }
            }
        }
        #[inline(always)]
        fn unmap_direct_range_2(ptes: &mut [PageTableEntry; 512], l: &[usize; 1], r: &[usize; 1]) {
            for pte in &mut ptes[l[0]..=r[0]] {
                debug_assert!(pte.is_leaf());
                *pte = PageTableEntry::empty();
            }
        }
    }

    /// if exists valid leaf, it will panic.
    pub fn free_user_directory_all(&mut self, allocator: &mut impl FrameAllocator) {
        let ubegin = UserAddr4K::null();
        let uend = UserAddr4K::user_max();
        let l = &ubegin.indexes();
        let r = &uend.sub_one_page().indexes();
        let ptes = self.root_pa().into_ref().as_pte_array_mut();
        let ua = free_user_directory_all_0(ptes, l, r, ubegin, allocator);
        assert_eq!(ua, uend);
        return;
        #[inline(always)]
        fn free_user_directory_all_0(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 3],
            r: &[usize; 3],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> UserAddr4K {
            let xbegin = &[0, 0];
            let xend = &[511, 511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    debug_assert!(
                        pte.is_directory(),
                        "free_user_directory_all: need directory but leaf"
                    );
                    let ptes = PageTable::ptes_from_pte(pte);
                    ua = free_user_directory_all_1(ptes, l, r, ua, allocator);
                    unsafe { pte.dealloc_by(allocator) }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r))
                }
            }
            ua
        }
        #[inline(always)]
        fn free_user_directory_all_1(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 2],
            r: &[usize; 2],
            mut ua: UserAddr4K,
            allocator: &mut impl FrameAllocator,
        ) -> UserAddr4K {
            let xbegin = &[0];
            let xend = &[511];
            for (i, pte) in &mut ptes[l[0]..=r[0]].iter_mut().enumerate() {
                let (l, r, _full) = PageTable::next_lr(l, r, xbegin, xend, i);
                if pte.is_valid() {
                    debug_assert!(
                        pte.is_directory(),
                        "free_user_directory_all: need directory but leaf"
                    );
                    let ptes = PageTable::ptes_from_pte(pte);
                    ua = free_user_directory_all_2(ptes, l, r, ua, allocator);
                    unsafe { pte.dealloc_by(allocator) }
                } else {
                    ua.add_page_assign(PageTable::indexes_diff(l, r))
                }
            }
            ua
        }
        #[inline(always)]
        fn free_user_directory_all_2(
            ptes: &mut [PageTableEntry; 512],
            l: &[usize; 1],
            r: &[usize; 1],
            mut ua: UserAddr4K,
            _allocator: &mut impl FrameAllocator,
        ) -> UserAddr4K {
            for pte in &mut ptes[l[0]..=r[0]] {
                assert!(
                    !pte.is_valid(),
                    "free_user_directory_all: exist valid leaf: {:?}",
                    ua
                );
                ua = ua.add_one_page();
            }
            ua
        }
    }
}
