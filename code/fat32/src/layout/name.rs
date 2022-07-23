use alloc::boxed::Box;
use ftl_util::{
    device::BlockDevice,
    time::{Instant, UtcTime},
};

use crate::tools::{self, Align8, CID};

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

impl Attr {
    pub fn new(dir: bool, read_only: bool, hidden: bool) -> Self {
        let mut v = Self::empty();
        if dir {
            v.insert(Attr::DIRECTORY);
        }
        if read_only {
            v.insert(Attr::READ_ONLY);
        }
        if hidden {
            v.insert(Attr::HIDDEN);
        }
        v
    }
    pub fn readonly(self) -> bool {
        self.contains(Self::READ_ONLY)
    }
    pub fn writable(self) -> bool {
        !self.readonly()
    }
    pub fn rw(self) -> (bool, bool) {
        (true, self.writable())
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
    // 这个位默认为0,只有短文件名时才有用.
    // 0x00时为文件名全大写
    // 0x08时为文件名全小写
    // 0x10时扩展名全大写
    // 0x00扩展名全小写
    // 0x18时为文件名全小写,扩展名全大写
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
    /// generate ".." "."
    pub(crate) fn init_dot_dir(&mut self, dot_n: usize, cid: CID, now: Instant) {
        debug_assert!(self.is_free());
        self.name[..dot_n].fill(b'.');
        self.name[dot_n..].fill(0x20);
        self.ext.fill(0x20);
        self.attributes = Attr::DIRECTORY;
        self.reversed = 0;
        self.init_time(now);
        self.set_cluster(cid);
        self.file_bytes = 0;
    }
    pub(crate) fn init_except_name(&mut self, cid: CID, file_bytes: u32, attr: Attr, now: Instant) {
        self.reversed = 0;
        self.set_cluster(cid);
        self.attributes = attr;
        self.init_time(now);
        self.file_bytes = file_bytes;
    }
    pub fn init_time(&mut self, now: Instant) {
        let utc_time = UtcTime::from_instant(now);
        self.set_access_time(&utc_time);
        self.set_access_time(&utc_time);
        self.set_modify_time(&utc_time);
    }
    pub fn is_free(&self) -> bool {
        [0x00, 0xE5].contains(&self.name[0])
    }
    pub fn is_dir(&self) -> bool {
        self.attributes.contains(Attr::DIRECTORY)
    }
    pub fn raw_name(&self) -> ([u8; 8], [u8; 3]) {
        (self.name, self.ext)
    }
    pub fn get_name<'a>(&self, buf: &'a mut [u8; 12]) -> &'a [u8] {
        let mut n = 0;
        let name_lower = self.reversed & 0x08 != 0;
        let ext_upper = self.reversed & 0x10 != 0;
        for &ch in &self.name {
            if ch == 0x20 {
                break;
            }
            buf[n] = match name_lower {
                true => ch.to_ascii_lowercase(),
                false => ch,
            };
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
            buf[n] = match ext_upper {
                true => ch.to_ascii_uppercase(),
                false => ch,
            };
            n += 1;
        }
        &buf[0..n]
    }
    pub fn checksum(&self) -> u8 {
        self.name
            .iter()
            .chain(self.ext.iter())
            .copied()
            .fold(0, |a, c| a.rotate_right(1).wrapping_add(c))
    }
    pub(crate) fn cid(&self) -> CID {
        CID((self.cluster_h16 as u32) << 16 | self.cluster_l16 as u32)
    }
    pub(crate) fn set_cluster(&mut self, cid: CID) {
        self.cluster_h16 = (cid.0 >> 16) as u16;
        self.cluster_l16 = cid.0 as u16;
    }
    pub fn file_bytes(&self) -> usize {
        self.file_bytes as usize
    }
    pub fn set_file_bytes(&mut self, bytes: usize) {
        self.file_bytes = bytes as u32
    }
    // -> (hms, date)
    fn time_tran(
        &(year, mount, day): &(usize, usize, usize),
        &(hour, min, sec): &(usize, usize, usize),
    ) -> (u16, u16) {
        let year = year - 1980;
        let sec = sec / 2;
        debug_assert!(year < (1 << 7));
        debug_assert!(mount <= 12);
        debug_assert!(day <= 31);
        debug_assert!(hour < 24);
        debug_assert!(min < 60);
        debug_assert!(sec < 30);
        let hms = hour << 11 | min << 5 | sec;
        let date = year << 9 | mount << 5 | day;
        (hms as u16, date as u16)
    }
    pub fn set_create_time(&mut self, utc_time: &UtcTime) {
        stack_trace!();
        debug_assert!(utc_time.nano < 1000_000_000);
        let (hms, date) = Self::time_tran(&utc_time.ymd, &utc_time.hms);
        self.create_ms = (utc_time.nano / 10_000_000) as u8;
        self.create_hms = hms;
        self.create_date = date;
    }
    pub fn set_access_time(&mut self, utc_time: &UtcTime) {
        self.access_date = Self::time_tran(&utc_time.ymd, &(0, 0, 0)).1;
    }
    pub fn set_modify_time(&mut self, utc_time: &UtcTime) {
        stack_trace!();
        let (hms, date) = Self::time_tran(&utc_time.ymd, &utc_time.hms);
        self.modify_hms = hms;
        self.modify_date = date;
    }
    pub fn access_time(&self) -> UtcTime {
        let mut time = UtcTime::base();
        time.set_ymd(self.access_date);
        time.set_hms(0);
        time.set_ms(0);
        time
    }
    pub fn modify_time(&self) -> UtcTime {
        let mut time = UtcTime::base();
        time.set_ymd(self.modify_date);
        time.set_hms(self.modify_hms);
        time.set_ms(0);
        time
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
pub(crate) struct RawLongName {
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
    pub fn zeroed() -> Align8<Self> {
        unsafe { Align8(core::mem::zeroed()) }
    }
    pub fn is_last(&self) -> bool {
        self.order & 0x40 != 0
    }
    pub fn order_num(&self) -> usize {
        debug_assert!(self.order & 0b11111 != 0);
        (self.order & 0b11111) as usize
    }
    pub fn set(&mut self, name: &[u16; 13], order: usize, last: bool, checksum: u8) {
        debug_assert!(order < 32);
        self.load_name(name);
        self.order = ((last as u8) << 6) | (order as u8);
        self.attributes = Attr::from_bits_truncate(0x0f);
        self.entry_type = 0;
        self.checksum = checksum;
        self.zero2 = [0; 2];
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
    pub fn checksum(&self) -> u8 {
        self.checksum
    }
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
pub(crate) union RawName {
    short: RawShortName,
    long: RawLongName,
}

pub(crate) enum Name<'a> {
    Short(&'a Align8<RawShortName>),
    Long(&'a Align8<RawLongName>),
}

impl RawName {
    pub fn short_init(&mut self) -> &mut Align8<RawShortName> {
        debug_assert!(self.is_free());
        unsafe { core::mem::transmute(&mut self.short) }
    }
    pub fn alloc_init(&mut self) {
        unsafe { self.short.name[0] = 0x00 };
    }
    pub fn from_short(short: &Align8<RawShortName>) -> Self {
        Self { short: **short }
    }
    pub fn from_long(long: &Align8<RawLongName>) -> Self {
        Self { long: **long }
    }
    pub fn cluster_init(buf: &mut [RawName]) {
        buf.iter_mut().for_each(|a| a.alloc_init())
    }
    pub fn set_free(&mut self) {
        debug_assert!(!self.is_free());
        unsafe { self.short.name[0] = 0xE5 };
    }
    pub fn set_short(&mut self, short: &Align8<RawShortName>) {
        self.short = **short;
    }
    pub fn is_long(&self) -> bool {
        !self.is_free() && self.attributes().bits() == 0x0F
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
    /// 如果是空闲项直接panic
    pub fn vaild_get(&self) -> Name {
        unsafe {
            debug_assert!(!self.is_free());
            if self.is_long() {
                debug_assert!(self.long.order & 0b1010_0000 == 0, "{:#x}", self.long.order);
                debug_assert!(self.long.order & 0b0001_1111 != 0, "{:#x}", self.long.order);
                Name::Long(core::mem::transmute(&self.long))
            } else {
                Name::Short(core::mem::transmute(&self.short))
            }
        }
    }
    pub fn get_short(&self) -> Option<&Align8<RawShortName>> {
        match self.get() {
            Some(Name::Short(s)) => Some(s),
            _ => None,
        }
    }
}

pub(crate) struct NameSet {
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
                        println!(
                            "{:3}     s:<{}> \tlen:{:<5} cid:{:#x} attr:{:#x} {:2x?}{:2x?}\n",
                            i,
                            str,
                            name.file_bytes(),
                            name.cid().0,
                            name.attributes,
                            name.name,
                            name.ext
                        );
                    }
                    Name::Long(name) => {
                        use alloc::string::String;
                        let mut buf = ['\0'; 13];
                        let str: String = name.get_name(&mut buf).iter().collect();
                        let mut raw = [0; 13];
                        name.store_name(&mut raw);
                        println!("{:3}  long:{:<13} \traw:{:4x?}", i, str, raw);
                    }
                },
            }
        }
    }
}
