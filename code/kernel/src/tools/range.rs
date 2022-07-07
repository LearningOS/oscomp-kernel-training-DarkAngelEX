use core::ops::Range;

use crate::memory::address::{PageCount, UserAddr4K};

pub type URange = Range<UserAddr4K>;

/// 限制range 返回值可能出现 start > end
pub fn range_limit<T: Ord + Copy>(src: Range<T>, area: Range<T>) -> Range<T> {
    let start = src.start.max(area.start);
    let end = src.end.min(area.end);
    Range { start, end }
}
pub fn range_null<T: Ord + Copy>(src: Range<T>) -> Range<T> {
    let start = src.start;
    Range { start, end: start }
}
/// outer覆盖inner时返回Ok
pub fn range_check<T: Ord + Copy>(outer: Range<T>, inner: Range<T>) -> Result<(), ()> {
    (outer.start <= inner.start && inner.start < inner.end && inner.end <= outer.end)
        .then_some(())
        .ok_or(())
}
pub fn ur_iter(URange { start, end }: URange) -> impl Iterator<Item = UserAddr4K> {
    struct CurEndIter {
        cur: UserAddr4K,
        end: UserAddr4K,
    }
    impl Iterator for CurEndIter {
        type Item = UserAddr4K;
        #[inline]
        fn next(&mut self) -> Option<Self::Item> {
            let x = self.cur;
            if x == self.end {
                return None;
            }
            self.cur.add_page_assign(PageCount(1));
            Some(x)
        }
    }
    CurEndIter { cur: start, end }
}
