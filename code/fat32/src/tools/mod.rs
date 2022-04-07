/// 逻辑扇区号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SID(pub u32);
/// 簇号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CID(pub u32);

/// 小端序加载
#[inline]
pub fn load_fn<T: Copy>(dst: &mut T, src: &[u8; 512], offset: &mut usize) {
    unsafe {
        let count = core::mem::size_of::<T>();
        core::ptr::copy_nonoverlapping(
            src.as_ptr().offset(*offset as isize),
            dst as *mut _ as *mut u8,
            count,
        );
        *offset += count;
    };
}
/// 小端序装载
#[inline]
pub fn store_fn<T: Copy>(src: &T, dst: &mut [u8; 512], offset: &mut usize) {
    unsafe {
        let count = core::mem::size_of::<T>();
        core::ptr::copy_nonoverlapping(
            src as *const _ as *const u8,
            dst.as_mut_ptr().offset(*offset as isize),
            count,
        );
        *offset += count;
    };
}
