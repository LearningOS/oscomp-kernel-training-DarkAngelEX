use alloc::boxed::Box;

use crate::{
    tools::{self, CID},
    BlockDevice,
};

use super::bpb::RawBPB;

bitflags! {
    pub struct Attr: u8 {
        const READ_ONLY = 1 << 0; // 只读
        const HIDDEN    = 1 << 1; // 隐藏
        const SYSTEM    = 1 << 2; // 系统
        const VOLUME_ID = 1 << 3; // 卷标
        const DIRECTORY = 1 << 4; // 目录
        const ARCHIVE   = 1 << 5; // 归档
    }
}

/// 文件名不足8则填充0x20 子目录扩展名填充0x20
///
/// 被删除后name[0]变为0xE5
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct RawShortName {
    pub name: [u8; 8],
    pub ext: [u8; 3],
    pub attributes: Attr, // 只读 隐藏 系统 卷标 目录 归档
    pub reversed: u8,
    pub create_ms: u8,    // 创建文件时间 单位为10ms
    pub create_hms: u16,  // [hour|minutes|seconds/2]=[5|6|5]
    pub create_date: u16, // [year-1980|mount|day]=[7|4|5]
    pub access_date: u16, // [year-1980|mount|day]=[7|4|5]
    pub cluster_h16: u16, // 文件起始簇号的高16位
    pub modify_hms: u16,  // [hour|minutes|seconds/2]=[5|6|5]
    pub modify_date: u16, // [year-1980|mount|day]=[7|4|5]
    pub cluster_l16: u16, // 文件起始簇号的低16位
    pub file_bytes: u32,  // 文件字节数
}

impl RawShortName {
    pub const fn zeroed() -> Self {
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
    pub fn get_name<'a>(&self, buf: &'a mut [u8; 12]) -> &'a [u8] {
        let mut n = 0;
        for &ch in &self.name {
            if ch == 0x20 {
                break;
            }
            buf[n] = ch;
            n += 1;
        }
        if self.ext[0] == 0x20 {
            return &buf[0..n];
        }
        buf[n] = '.' as u8;
        n += 1;
        for &ch in &self.ext {
            if ch == 0x20 {
                break;
            }
            buf[n] = ch;
            n += 1;
        }
        &buf[0..n]
    }
}

/// 长文件名被放置在短文件名之前 存放13个unicode char
///
/// 最长文件名为 31*13 = 403
///
/// 填充值为 0,-1,-1, ...
///
/// 使用packed后rust将自行将操作转化为位运算, 不要去获取引用!
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct RawLongName {
    order: u8,        // [0|last|reverse|order]=[1|1|1|5] max order = 31 start = 1
    p1: [u16; 5],     // 5 unicode char
    attributes: Attr, // always 0x0F for long name entry
    entry_type: u8,   // always 0x0
    checksum: u8,     // 短文件名校验值
    p2: [u16; 6],     // 6 unicode char
    zero2: [u8; 2],   // always 0x0
    p3: [u16; 2],     // 2 unicode char
}

impl RawLongName {
    pub fn is_last(&self) -> bool {
        self.order & 0x40 != 0
    }
    pub fn order_num(&self) -> usize {
        debug_assert!(self.order & 0b11111 != 0);
        (self.order & 0b11111) as usize
    }
    pub fn set(&mut self, name: &[u16; 13], order: usize, last: bool) {
        todo!()
    }
    pub fn store_name(&self, dst: &mut [u16; 13]) {
        for i in 0..5 {
            dst[i] = self.p1[i];
        }
        for i in 0..6 {
            dst[i + 5] = self.p2[i];
        }
        for i in 0..2 {
            dst[i + 11] = self.p3[i];
        }
    }
    pub fn load_name(&mut self, src: &[u16; 13]) {
        for i in 0..5 {
            self.p1[i] = src[i];
        }
        for i in 0..6 {
            self.p2[i] = src[i + 5];
        }
        for i in 0..2 {
            self.p3[i] = src[i + 11];
        }
    }
    pub fn get_name<'a>(&self, buf: &'a mut [char; 13]) -> &'a [char] {
        let mut t = [0; 13];
        self.store_name(&mut t);
        let mut n = 0;
        for c in char::decode_utf16(t).map(|c| c.unwrap()) {
            if c == '\0' {
                break;
            }
            buf[n] = c;
            n += 1;
        }
        &buf[..n]
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union RawName {
    short: RawShortName,
    long: RawLongName,
}

pub enum Name<'a> {
    Short(&'a RawShortName),
    Long(&'a RawLongName),
}

impl RawName {
    pub fn alloc_init(&mut self) {
        unsafe { self.short.name[0] = 0x00 };
    }
    pub fn cluster_init(buf: &mut [RawName]) {
        buf.iter_mut().for_each(|a| a.alloc_init())
    }
    pub fn set_long(&mut self, name: &[u16; 13], order: usize, last: bool) {
        unsafe {
            self.long.set(name, order, last);
        }
    }
    pub fn set_short(&mut self, short: &RawShortName) {
        unsafe {
            self.short = *short;
        }
    }
    pub fn zeroed() -> Self {
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
    pub fn is_long(&self) -> bool {
        self.attributes().bits() == 0x0F
    }
    pub fn attributes(&self) -> Attr {
        unsafe { self.short.attributes }
    }
    pub fn is_free(&self) -> bool {
        unsafe { [0x00, 0xE5].contains(&self.long.order) }
    }
    /// 如果此项为空闲项, 返回None
    pub fn get(&self) -> Option<Name> {
        if self.is_free() {
            return None;
        }
        Some(self.vaild_get())
    }
    pub fn vaild_get(&self) -> Name {
        unsafe {
            debug_assert!(![0x00, 0xE5].contains(&self.long.order));
            if self.is_long() {
                debug_assert!(self.long.order & 0b1010_0000 == 0);
                debug_assert!(self.long.order & 0b0001_1111 != 0);
                Name::Long(&self.long)
            } else {
                Name::Short(&self.short)
            }
        }
    }
}

pub struct NameSet {
    names: Box<[RawName]>,
}

impl NameSet {
    pub fn new(bpb: &RawBPB) -> Self {
        let size = core::mem::size_of::<RawName>();
        debug_assert!(size.is_power_of_two());
        Self {
            names: unsafe { Box::new_zeroed_slice(bpb.cluster_bytes / size).assume_init() },
        }
    }
    pub async fn load(&mut self, bpb: &RawBPB, cid: CID, device: &dyn BlockDevice) {
        let sid = bpb.cid_transform(cid);
        let buf = tools::to_bytes_slice_mut(&mut self.names);
        device.read_block(sid.0 as usize, buf).await.unwrap();
    }
    pub fn show(&self, mut n: usize) {
        if n == 0 {
            n = usize::MAX;
        }
        for (i, name) in self.names.iter().take(n).enumerate() {
            match name.get() {
                None => {
                    println!("{:3}  None", i);
                }
                Some(name) => match name {
                    Name::Short(name) => {
                        let mut buf = [0; 12];
                        let str =
                            unsafe { core::str::from_utf8_unchecked(name.get_name(&mut buf)) };
                        println!("{:3}     s:{}", i, str);
                    }
                    Name::Long(name) => {
                        use alloc::string::String;
                        let mut buf = ['\0'; 13];
                        let str: String = name.get_name(&mut buf).iter().collect();
                        println!("{:3}  long:{}", i, str);
                    }
                },
            }
        }
    }
}
