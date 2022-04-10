pub mod xasync;

/// 扇区号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SID(pub u32);
/// 簇号
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CID(pub u32);

impl CID {
    pub fn set_free(&mut self) {
        self.0 = 0;
    }
    pub fn set_next(&mut self, next: CID) {
        debug_assert!(next.is_next());
        self.0 = next.0;
    }
    pub fn set_last(&mut self) {
        self.0 = 0x0FFFFFFF;
    }
    pub fn is_free(self) -> bool {
        matches!(self.status(), ClStatus::Free)
    }
    pub fn is_using(self) -> bool {
        self.is_next() || self.is_last()
    }
    pub fn is_last(self) -> bool {
        matches!(self.status(), ClStatus::Last)
    }
    pub fn is_bad(self) -> bool {
        matches!(self.status(), ClStatus::Bad)
    }
    pub fn is_next(self) -> bool {
        matches!(self.status(), ClStatus::Next(_))
    }
    /// 保证self不为0
    pub fn next(self) -> Option<CID> {
        match self.status() {
            ClStatus::Next(cid) => Some(cid),
            _ => None,
        }
    }
    pub fn status(self) -> ClStatus {
        match self.0 {
            0x0 => ClStatus::Free,
            0x1 => ClStatus::Reverse,
            0x2..0x0FFFFFF0 => ClStatus::Next(self),
            0x0FFFFFF0..0x0FFFFFF7 => ClStatus::Reverse,
            0x0FFFFFF7 => ClStatus::Bad,
            0x0FFFFFF8..0x10000000 => ClStatus::Last,
            v => panic!("error CID:{:#x}", v),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClStatus {
    Free,
    Reverse,
    Next(CID),
    Bad,
    Last,
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

pub fn from_bytes_slice<T: Copy>(s: &[u8]) -> &[T] {
    unsafe {
        if cfg!(debug_assert) {
            let (a, _b, c) = s.align_to::<T>();
            assert!(a.is_empty() && c.is_empty());
        }
        let len = s.len() / core::mem::size_of::<T>();
        &*core::slice::from_raw_parts(s.as_ptr() as *const T, len)
    }
}
pub fn from_bytes_slice_mut<T: Copy>(s: &mut [u8]) -> &mut [T] {
    unsafe {
        if cfg!(debug_assert) {
            let (a, _b, c) = s.align_to::<T>();
            assert!(a.is_empty() && c.is_empty());
        }
        let len = s.len() / core::mem::size_of::<T>();
        &mut *core::slice::from_raw_parts_mut(s.as_mut_ptr() as *mut T, len)
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
