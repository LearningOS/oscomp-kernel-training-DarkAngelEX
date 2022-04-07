use crate::tools;

/// 文件名不足8则填充0x20 子目录扩展名填充0x20
///
/// 被删除后name[0]变为0xE5
#[repr(C, packed)]
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

/// 长文件名被放置在短文件名之前 存放13个unicode char
///
/// 最长文件名为 31*13 = 403
///
/// 填充值为 0,-1,-1, ...
///
/// 使用packed后rust将自行将操作转化为位运算, 不要去获取引用!
#[repr(C, packed)]
pub struct RawLongName {
    order: u8,      // [0|last|reverse|order]=[1|1|1|5] max order = 31 start = 1
    p1: [u16; 5],   // 5 unicode char
    attributes: u8, // always 0x0F for long name entry
    entry_type: u8, // always 0x0
    checksum: u8,   // 段文件名校验值
    p2: [u16; 6],   // 6 unicode char
    zero2: [u8; 2], // always 0x0
    p3: [u16; 2],   // 2 unicode char
}

impl RawLongName {
    pub fn load_name(&self, dst: &mut [u16; 13]) {
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
    pub fn store_name(&mut self, src: &[u16; 13]) {
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
}
