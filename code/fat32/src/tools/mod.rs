/// 扇区号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SID(pub u32);
/// 簇号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CID(pub u32);

impl CID {
    pub fn is_free(self) -> bool {
        self.0 == 0
    }
    pub fn is_last(self) -> bool {
        self.0 >= 0x0FFFFFF8
    }
    pub fn is_bad(self) -> bool {
        self.0 == 0x0FFFFFF7
    }
    /// 保证self不为0
    pub fn next(self) -> Option<CID> {
        if self.0 < 0x0FFFFFF7 {
            return Some(self);
        }
        None
    }
}

pub fn to_bytes_slice<T: Copy>(s: &[T]) -> &[u8] {
    unsafe {
        let len = s.len() * core::mem::size_of::<T>();
        &*core::slice::from_raw_parts(s.as_ptr() as *const u8, len)
    }
}
pub fn to_bytes_slice_mut<T: Copy>(s: &mut [T]) -> &mut [u8] {
    unsafe {
        let len = s.len() * core::mem::size_of::<T>();
        &mut *core::slice::from_raw_parts_mut(s.as_mut_ptr() as *mut u8, len)
    }
}

/// 小端序加载
#[inline]
pub fn load_fn<T: Copy>(dst: &mut T, src: &[u8], offset: &mut usize) {
    unsafe {
        let count = core::mem::size_of::<T>();
        core::ptr::copy_nonoverlapping(&src[*offset], dst as *mut _ as *mut u8, count);
        *offset += count;
    };
}
/// 小端序装载
#[inline]
pub fn store_fn<T: Copy>(src: &T, dst: &mut [u8], offset: &mut usize) {
    unsafe {
        let count = core::mem::size_of::<T>();
        core::ptr::copy_nonoverlapping(src as *const _ as *const u8, &mut dst[*offset], count);
        *offset += count;
    };
}
