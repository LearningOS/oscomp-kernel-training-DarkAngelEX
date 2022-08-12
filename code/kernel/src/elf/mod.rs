//! 一个异步环境的elf解析器

use core::mem;

use alloc::vec::Vec;
use ftl_util::error::{SysError, SysR};
use vfs::{File, VfsFile};
use xmas_elf::{
    header::{Class, HeaderPt1, HeaderPt2_, Machine_, Type_},
    program::{Flags, ProgramHeader32, ProgramHeader64, Type},
    sections::SectionHeader_,
};

pub async fn parse(file: &VfsFile) -> SysR<ElfAnalyzer<'_>> {
    const PH1_SIZE: usize = mem::size_of::<HeaderPt1>();
    const PH2_SIZE_32: usize = mem::size_of::<HeaderPt2_<u32>>();
    const PH2_SIZE_64: usize = mem::size_of::<HeaderPt2_<u64>>();
    type Ph1Arr = [u8; PH1_SIZE];
    let mut pt1: HeaderPt1 = unsafe { core::mem::MaybeUninit::uninit().assume_init() };
    let ph1_arr: &mut Ph1Arr = unsafe { core::mem::transmute(&mut pt1) };
    let n = file.read_at(0, ph1_arr).await?;
    if n < PH1_SIZE {
        println!("elf parse error: file too short");
        return Err(SysError::EFAULT);
    }
    if pt1.magic != [0x7f, b'E', b'L', b'F'] {
        println!("elf parse error: magic error");
        return Err(SysError::EFAULT);
    }
    let pt2: HeaderPt2 = match pt1.class() {
        Class::None | Class::Other(_) => {
            println!("Invalid ELF class");
            return Err(SysError::EFAULT);
        }
        Class::ThirtyTwo => {
            let mut buf: [u8; PH2_SIZE_32] = [0; _];
            let n = file.read_at(PH1_SIZE, &mut buf).await?;
            if n < PH2_SIZE_32 {
                println!("File is shorter than ELF 32 headers");
                return Err(SysError::EFAULT);
            }
            let head: HeaderPt2_<u32> = unsafe { mem::transmute(buf) };
            HeaderPt2::from_32header(&head)
        }
        Class::SixtyFour => {
            let mut buf: [u8; PH2_SIZE_64] = [0; _];
            let n = file.read_at(PH1_SIZE, &mut buf).await?;
            if n < PH2_SIZE_64 {
                println!("File is shorter than ELF 64 headers");
                return Err(SysError::EFAULT);
            }
            unsafe { mem::transmute(buf) }
        }
    };
    Ok(ElfAnalyzer { file, pt1, pt2 })
}

pub struct ElfAnalyzer<'a> {
    file: &'a VfsFile,
    pub pt1: HeaderPt1,
    pub pt2: HeaderPt2,
}

impl<'a> ElfAnalyzer<'a> {
    pub fn ph_count(&self) -> u16 {
        self.pt2.ph_count
    }
    pub async fn program_header(&self, index: u16) -> SysR<Segment> {
        debug_assert!(index < self.ph_count());
        let pt2 = &self.pt2;
        if pt2.ph_offset == 0 || pt2.ph_entry_size == 0 {
            println!("There are no program headers in this file");
            return Err(SysError::EFAULT);
        }

        let start = pt2.ph_offset as usize + index as usize * pt2.ph_entry_size as usize;
        let size = pt2.ph_entry_size as usize;
        let mut buf: [u8; mem::size_of::<ProgramHeader64>()] = [0; _];
        let n = self.file.read_at(start, &mut buf[..size]).await?;
        if n < size {
            println!("file to short!");
            return Err(SysError::EFAULT);
        }

        match self.pt1.class() {
            Class::ThirtyTwo => Ok(Segment::Ph32(unsafe { mem::transmute_copy(&buf) })),
            Class::SixtyFour => Ok(Segment::Ph64(unsafe { mem::transmute_copy(&buf) })),
            Class::None | Class::Other(_) => unreachable!(),
        }
    }
    pub async fn find_section_by_name(&self, _nama: &str) -> Option<SectionHeader> {
        None
        // todo!()
    }
}

#[derive(Debug)]
pub enum Segment {
    Ph32(ProgramHeader32),
    Ph64(ProgramHeader64),
}

impl Segment {
    pub fn get_type(&self) -> Result<Type, &'static str> {
        match self {
            Segment::Ph32(ph) => ph.get_type(),
            Segment::Ph64(ph) => ph.get_type(),
        }
    }
    /// R W X
    pub fn flags(&self) -> Flags {
        match self {
            Segment::Ph32(ph) => ph.flags,
            Segment::Ph64(ph) => ph.flags,
        }
    }
    /// 这个段的虚拟地址空间
    pub fn virtual_addr(&self) -> usize {
        match self {
            Segment::Ph32(ph) => ph.virtual_addr as usize,
            Segment::Ph64(ph) => ph.virtual_addr as usize,
        }
    }
    /// 这个段的虚拟地址空间长度
    pub fn mem_size(&self) -> usize {
        match self {
            Segment::Ph32(ph) => ph.mem_size as usize,
            Segment::Ph64(ph) => ph.mem_size as usize,
        }
    }
    /// 这个段的在文件内的数据长度, 长度不足则填0
    pub fn file_size(&self) -> usize {
        match self {
            Segment::Ph32(ph) => ph.file_size as usize,
            Segment::Ph64(ph) => ph.file_size as usize,
        }
    }
    /// 这个段对应数据在文件内的偏移
    pub fn offset(&self) -> usize {
        match self {
            Segment::Ph32(ph) => ph.offset as usize,
            Segment::Ph64(ph) => ph.offset as usize,
        }
    }
}

pub struct SegmentIter<'a> {
    file: &'a VfsFile,
    bit64: bool,
}

#[repr(C)]
pub struct HeaderPt2 {
    pub type_: Type_,
    pub machine: Machine_,
    pub version: u32,
    pub entry_point: usize,
    pub ph_offset: usize,
    pub sh_offset: usize,
    pub flags: u32,
    pub header_size: u16,
    pub ph_entry_size: u16,
    pub ph_count: u16,
    pub sh_entry_size: u16,
    pub sh_count: u16,
    pub sh_str_index: u16,
}

impl HeaderPt2 {
    pub fn from_32header(h: &HeaderPt2_<u32>) -> Self {
        Self {
            type_: h.type_,
            machine: h.machine,
            version: h.version,
            entry_point: h.entry_point as _,
            ph_offset: h.ph_offset as _,
            sh_offset: h.sh_offset as _,
            flags: h.flags,
            header_size: h.header_size,
            ph_entry_size: h.ph_entry_size,
            ph_count: h.ph_count,
            sh_entry_size: h.sh_entry_size,
            sh_count: h.sh_count,
            sh_str_index: h.sh_str_index,
        }
    }
}

pub enum SectionHeader {
    Sh32(SectionHeader_<u32>),
    Sh64(SectionHeader_<u64>),
}

impl SectionHeader {
    pub async fn raw_data(&self, _elf: &ElfAnalyzer<'_>) -> Vec<u8> {
        todo!()
    }
}
