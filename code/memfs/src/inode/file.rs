use alloc::vec::Vec;
use ftl_util::error::SysError;

pub struct FileInode {
    data: Vec<u8>,
}

impl FileInode {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }
    pub fn clear(&mut self) {
        self.data.clear()
    }
    pub fn read_at(&mut self, buf: &mut [u8], offset: usize) -> Result<usize, SysError> {
        if offset > self.data.len() {
            return Err(SysError::EINVAL);
        }
        let n = self.data[offset..].len().min(buf.len());
        buf[..n].copy_from_slice(&self.data[offset..offset + n]);
        Ok(n)
    }
    pub fn write_at(&mut self, buf: &[u8], offset: usize) -> Result<usize, SysError> {
        let end = offset + buf.len();
        if offset + buf.len() > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[offset..end].copy_from_slice(buf);
        Ok(buf.len())
    }
}
