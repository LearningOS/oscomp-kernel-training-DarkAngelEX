#![allow(dead_code)]
use core::{fmt::Debug, marker::PhantomData};

/// big end
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Fdt32(u32);

impl Debug for Fdt32 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("Fdt32 {}", self.small_end()))
    }
}

impl Fdt32 {
    pub fn small_end(&self) -> u32 {
        let x = self.0;
        let [a, b, c, d] = [x >> 0, x >> 8, x >> 16, x >> 24].map(|a| a & 0xff);
        a << 24 | b << 16 | c << 8 | d << 0
    }
}

pub struct FdtHeader {
    pub magic: Fdt32,             /* magic word FDT_MAGIC */
    pub totalsize: Fdt32,         /* total size of DT block */
    pub off_dt_struct: Fdt32,     /* offset to structure */
    pub off_dt_strings: Fdt32,    /* offset to strings */
    pub off_mem_rsvmap: Fdt32,    /* offset to memory reserve map */
    pub version: Fdt32,           /* format version */
    pub last_comp_version: Fdt32, /* last compatible version */
    /* version 2 fields below */
    pub boot_cpuid_phys: Fdt32, /* Which physical CPU id we're booting on */
    /* version 3 fields below */
    pub size_dt_strings: Fdt32, /* size of the strings block */
    /* version 17 fields below */
    pub size_dt_struct: Fdt32, /* size of the structure block */
}

#[allow(non_camel_case_types)]
enum Tag {
    FDT_BEGIN_NODE = 0x1,
    FDT_END_NODE = 0x2,
    FDT_PROP = 0x3,
    FDT_NOP = 0x4,
    FDT_END = 0x9,
}
pub struct FdtNodeHeader {
    pub tag: Fdt32,
    pub name: PhantomData<u8>,
}

pub struct FdtProperty {
    pub tag: Fdt32,
    pub len: Fdt32,
    pub nameoff: Fdt32,
    pub data: PhantomData<u8>,
}

impl Debug for FdtHeader {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!(
            "fdt header:
    magic:            {:#x}
    totalsize:        {:}
    off_dt_struct:    {:}
    off_dt_strings:   {:}
    off_mem_rsvmap:   {:}
    version:          {:}
    last_comp_version:{:}
    boot_cpuid_phys:  {:}
    size_dt_strings:  {:}
    size_dt_struct:   {:}
        ",
            self.magic.small_end(),
            self.totalsize.small_end(),
            self.off_dt_struct.small_end(),
            self.off_dt_strings.small_end(),
            self.off_mem_rsvmap.small_end(),
            self.version.small_end(),
            self.last_comp_version.small_end(),
            self.boot_cpuid_phys.small_end(),
            self.size_dt_strings.small_end(),
            self.size_dt_struct.small_end(),
        ))
    }
}
