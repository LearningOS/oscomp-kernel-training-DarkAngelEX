//! read 4K from iterator

use crate::config::PAGE_SIZE;

pub trait FrameDataIter {
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// if empty will return 0
    fn len(&self) -> usize;
    /// if no data exists, fill zero and return Err(())
    fn write_to(&mut self, dst: &mut [u8; 4096]) -> Result<(), ()>;
}

pub struct SliceFrameDataIter<'a> {
    data: &'a [u8],
}
impl<'a> SliceFrameDataIter<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data }
    }
}
impl<'a> FrameDataIter for SliceFrameDataIter<'a> {
    fn len(&self) -> usize {
        self.data.len()
    }
    /// after end, write zero to it.
    fn write_to(&mut self, dst: &mut [u8; 4096]) -> Result<(), ()> {
        match self.next() {
            Some(src) => {
                dst[..src.len()].copy_from_slice(src);
                dst[src.len()..].fill(0);
                Ok(())
            }
            None => {
                dst.fill(0);
                Err(())
            }
        }
    }
}
impl<'a> SliceFrameDataIter<'a> {
    fn next(&mut self) -> Option<&[u8]> {
        if self.data.is_empty() {
            return None;
        }
        let len = self.data.len();
        let (l, r) = self.data.split_at(PAGE_SIZE.min(len));
        self.data = r;
        Some(l)
    }
}

pub struct ZeroFrameDataIter;

impl FrameDataIter for ZeroFrameDataIter {
    fn len(&self) -> usize {
        0
    }
    fn write_to(&mut self, dst: &mut [u8; 4096]) -> Result<(), ()> {
        dst.fill(0);
        Ok(())
    }
}

/// write nothing
pub struct NullFrameDataIter;

impl FrameDataIter for NullFrameDataIter {
    fn len(&self) -> usize {
        0
    }
    fn write_to(&mut self, _dst: &mut [u8; 4096]) -> Result<(), ()> {
        Err(())
    }
}
