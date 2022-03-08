use crate::{config::PAGE_SIZE, memory::allocator::frame::global::FrameTracker};

use super::{UserData, UserDataMut};

// readonly, forbid write.
pub struct UserData4KIter<'a> {
    data: &'a UserData<u8>,
    idx: usize,
    buffer: FrameTracker,
}

impl<'a> Iterator for UserData4KIter<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.idx;
        let start = idx * PAGE_SIZE;
        if self.data.len() <= start {
            return None;
        }
        self.idx += 1;
        let end = self.data.len().min(start + PAGE_SIZE);
        let len = end - start;
        let src = &*self.data.access();
        let dst = self.buffer.data().as_bytes_array_mut();
        dst[0..len].copy_from_slice(&src[start..end]);
        Some(&dst[0..len])
    }
}

impl<'a> UserData4KIter<'a> {
    pub fn new(data: &'a UserData<u8>, buffer: FrameTracker) -> Self {
        Self {
            data,
            idx: 0,
            buffer,
        }
    }
}

/// writonly, forbid read.
pub struct UserDataMut4KIter<'a> {
    data: &'a UserDataMut<u8>,
    idx: usize,
    buffer: FrameTracker,
}

impl<'a> UserDataMut4KIter<'a> {
    pub fn new(data: &'a UserDataMut<u8>, buffer: FrameTracker) -> Self {
        Self {
            data,
            idx: 0,
            buffer,
        }
    }
}

impl<'a> Iterator for UserDataMut4KIter<'a> {
    type Item = &'a mut [u8];
    // write prev buffer into user_range.
    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.idx;
        let start = idx * PAGE_SIZE;
        // first return None satisfy self.data.len() <= idx * PAGE_SIZE
        if self.data.len() + PAGE_SIZE <= start {
            return None;
        }
        self.idx += 1;
        if idx != 0 {
            let mut xdst = self.data.access_mut();
            let src = self.buffer.data().as_bytes_array_mut();
            let dst = &mut *xdst;
            let src_end = start.min(dst.len());
            let src_len = src_end - (start - PAGE_SIZE);
            dst[start - PAGE_SIZE..src_end].copy_from_slice(&src[0..src_len]);
            if self.data.len() <= start {
                return None;
            }
        }
        let len = (self.data.len() - start).min(PAGE_SIZE);
        Some(&mut self.buffer.data().as_bytes_array_mut()[0..len])
    }
}
