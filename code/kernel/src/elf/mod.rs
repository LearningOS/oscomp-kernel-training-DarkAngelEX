//! 一个异步环境的elf解析器

use core::mem;

use alloc::{string::String, vec::Vec};
use ftl_util::error::{SysError, SysR};
use vfs::{File, VfsFile};
use xmas_elf::{
    header::{Class, HeaderPt1, HeaderPt2_, Machine_, Type_},
    program::{Flags, ProgramHeader32, ProgramHeader64, Type},
    sections::{ShType, SHT_HIOS, SHT_HIPROC, SHT_HIUSER, SHT_LOOS, SHT_LOPROC, SHT_LOUSER},
};

pub async fn parse(file: &VfsFile) -> SysR<ElfAnalyzer<'_>> {
    stack_trace!();
    const PH1_SIZE: usize = mem::size_of::<HeaderPt1>();
    const PH2_SIZE_32: usize = mem::size_of::<HeaderPt2_<u32>>();
    const PH2_SIZE_64: usize = mem::size_of::<HeaderPt2_<u64>>();
    type Ph1Arr = [u8; PH1_SIZE];
    #[allow(clippy::uninit_assumed_init)]
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
    pub async fn section_header(&self, index: u16) -> SysR<SectionHeader> {
        stack_trace!();
        assert!(index < self.pt2.sh_count);
        let start = index as usize * self.pt2.sh_entry_size as usize + self.pt2.sh_offset as usize;
        let size = self.pt2.sh_entry_size as usize;
        let mut buf: [u8; mem::size_of::<SectionHeader_<u64>>()] = [0; _];
        let n = self.file.read_at(start, &mut buf[..size]).await?;
        if n < size {
            println!("file to short!");
            return Err(SysError::EFAULT);
        }
        match self.pt1.class() {
            Class::ThirtyTwo => Ok(SectionHeader::Sh32(unsafe { mem::transmute_copy(&buf) })),
            Class::SixtyFour => Ok(SectionHeader::Sh64(unsafe { mem::transmute_copy(&buf) })),
            Class::None | Class::Other(_) => unreachable!(),
        }
    }
    pub async fn find_section_by_name(&self, name: &str) -> SysR<Option<SectionHeader>> {
        stack_trace!();
        let count = self.pt2.sh_count;
        for i in 0..count {
            let section = self.section_header(i).await?;
            if let Ok(sec_name) = section.get_name(self).await {
                if sec_name == name {
                    return Ok(Some(section));
                }
            }
        }
        Ok(None)
    }
    async fn get_shstr(&self, index: u32) -> SysR<String> {
        stack_trace!();
        let offset = self.section_header(self.pt2.sh_str_index).await?.offset();
        let mut buf = [0; 128];
        let mut cur = offset + index as usize;
        let mut s = Vec::new();
        'outer: loop {
            let n = self.file.read_at(cur, &mut buf).await?;
            for &c in &buf[..n] {
                if c == 0 {
                    break 'outer;
                }
                s.push(c);
            }
            if n < buf.len() {
                break;
            }
            cur += n;
        }
        String::from_utf8(s).map_err(|_e| SysError::EFAULT)
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

#[derive(Debug)]
pub enum SectionHeader {
    Sh32(SectionHeader_<u32>),
    Sh64(SectionHeader_<u64>),
}

#[derive(Debug)]
#[repr(C)]
pub struct SectionHeader_<P> {
    name: u32,
    type_: ShType_,
    flags: P,
    address: P,
    offset: P,
    size: P,
    link: u32,
    info: u32,
    align: P,
    entry_size: P,
}

impl SectionHeader {
    pub async fn raw_data(&self, elf: &ElfAnalyzer<'_>) -> SysR<Vec<u8>> {
        stack_trace!();
        assert_ne!(self.get_type().unwrap(), ShType::Null);
        let mut v = Vec::new();
        v.resize(self.size(), 0);
        let n = elf.file.read_at(self.offset(), &mut v).await?;
        if n != v.len() {
            return Err(SysError::EFAULT);
        }
        Ok(v)
    }
    pub fn get_type(&self) -> SysR<ShType> {
        self.type_().as_sh_type()
    }
    fn type_(&self) -> ShType_ {
        match self {
            SectionHeader::Sh32(s) => s.type_,
            SectionHeader::Sh64(s) => s.type_,
        }
    }
    fn name(&self) -> u32 {
        match self {
            SectionHeader::Sh32(s) => s.name,
            SectionHeader::Sh64(s) => s.name,
        }
    }
    fn size(&self) -> usize {
        match self {
            SectionHeader::Sh32(s) => s.size as _,
            SectionHeader::Sh64(s) => s.size as _,
        }
    }
    fn offset(&self) -> usize {
        match self {
            SectionHeader::Sh32(s) => s.offset as _,
            SectionHeader::Sh64(s) => s.offset as _,
        }
    }
    pub async fn get_name(&self, elf: &ElfAnalyzer<'_>) -> SysR<String> {
        stack_trace!();
        match self.get_type()? {
            ShType::Null => Err(SysError::EFAULT),
            _ => elf.get_shstr(self.name()).await,
        }
    }
}

#[derive(Debug)]
#[derive(Copy, Clone)]
pub struct ShType_(u32);

impl ShType_ {
    fn as_sh_type(self) -> SysR<ShType> {
        match self.0 {
            0 => Ok(ShType::Null),
            1 => Ok(ShType::ProgBits),
            2 => Ok(ShType::SymTab),
            3 => Ok(ShType::StrTab),
            4 => Ok(ShType::Rela),
            5 => Ok(ShType::Hash),
            6 => Ok(ShType::Dynamic),
            7 => Ok(ShType::Note),
            8 => Ok(ShType::NoBits),
            9 => Ok(ShType::Rel),
            10 => Ok(ShType::ShLib),
            11 => Ok(ShType::DynSym),
            // sic.
            14 => Ok(ShType::InitArray),
            15 => Ok(ShType::FiniArray),
            16 => Ok(ShType::PreInitArray),
            17 => Ok(ShType::Group),
            18 => Ok(ShType::SymTabShIndex),
            st if st >= SHT_LOOS && st <= SHT_HIOS => Ok(ShType::OsSpecific(st)),
            st if st >= SHT_LOPROC && st <= SHT_HIPROC => Ok(ShType::ProcessorSpecific(st)),
            st if st >= SHT_LOUSER && st <= SHT_HIUSER => Ok(ShType::User(st)),
            _ => Err(SysError::EFAULT),
        }
    }
}
