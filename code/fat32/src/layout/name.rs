use alloc::boxed::Box;

use crate::{
    tools::{self, CID},
    BlockDevice,
};

use super::bpb::RawBPB;

/// 文件名不足8则填充0x20 子目录扩展名填充0x20
///
/// 被删除后name[0]变为0xE5
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct RawShortName {
    name: [u8; 8],
    ext: [u8; 3],
    attributes: u8, // 只读 隐藏 系统 卷标 子目录 归档
    reversed: u8,
    create_ms: u8,    // 创建文件时间 单位为10ms
    create_hms: u16,  // [hour|minutes|seconds/2]=[5|6|5]
    create_date: u16, // [year-1980|mount|day]=[7|4|5]
    access_date: u16, // [year-1980|mount|day]=[7|4|5]
    cluster_h16: u16, // 文件起始簇号的高16位
    modify_hms: u16,  // [hour|minutes|seconds/2]=[5|6|5]
    modify_date: u16, // [year-1980|mount|day]=[7|4|5]
    cluster_l16: u16, // 文件起始簇号的低16位
    file_bytes: u32,  // 文件字节数
}

impl RawShortName {
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
    order: u8,      // [0|last|reverse|order]=[1|1|1|5] max order = 31 start = 1
    p1: [u16; 5],   // 5 unicode char
    attributes: u8, // always 0x0F for long name entry
    entry_type: u8, // always 0x0
    checksum: u8,   // 短文件名校验值
    p2: [u16; 6],   // 6 unicode char
    zero2: [u8; 2], // always 0x0
    p3: [u16; 2],   // 2 unicode char
}

impl RawLongName {
    pub fn is_last(&self) -> bool {
        self.attributes & 0x40 != 0
    }
    pub fn order_num(&self) -> usize {
        debug_assert!(self.order & 0x1F != 0);
        (self.order & 0x1F) as usize
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
        for (dst, src) in buf.iter_mut().zip(t) {
            if src == 0 {
                break;
            }
            unsafe { *dst = char::from_u32_unchecked(src as u32) };
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
    pub fn empty() -> Self {
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
    pub fn is_long(&self) -> bool {
        unsafe { self.long.attributes == 0x0F }
    }
    /// 如果此项为空闲项, 返回None
    pub fn get(&self) -> Option<Name> {
        unsafe {
            let order = self.long.order;
            if [0x00, 0xE5].contains(&order) {
                return None;
            }
            Some(self.vaild_get())
        }
    }
    pub fn vaild_get(&self) -> Name {
        unsafe {
            debug_assert!(![0x00, 0xE5].contains(&self.long.order));

            if self.is_long() {
                debug_assert!(self.long.order & 0xA0 == 0);
                debug_assert!(self.long.order & 0x1F != 0);
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
    pub async fn load(&mut self, bpb: &RawBPB, cid: CID, device: &impl BlockDevice) {
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
                        let str = core::str::from_utf8(name.get_name(&mut buf)).unwrap();
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
