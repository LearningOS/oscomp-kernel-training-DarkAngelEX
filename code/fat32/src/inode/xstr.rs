use alloc::{string::String, vec::Vec};
use ftl_util::error::SysError;

use crate::{layout::name::RawShortName, tools::Align8};

/// 长文件名反序 最后一项在前
pub fn utf16_to_string<'a>(src: impl DoubleEndedIterator<Item = &'a [u16; 13]>) -> String {
    let u16_iter = src
        .rev()
        .flat_map(|&s| s.into_iter())
        .take_while(|&s| s != 0x00)
        .into_iter();
    char::decode_utf16(u16_iter)
        .map(|r| r.unwrap_or(core::char::REPLACEMENT_CHARACTER))
        .collect()
}

/// 删除头尾的空格和尾部的点
fn name_trim(str: &str) -> &str {
    if [".", ".."].contains(&str) {
        return str;
    }
    str.trim_start_matches(' ').trim_end_matches(&[' ', '.'])
}

/// 检测名字的合法性, 并进行trim
///
/// 长度只进行粗略检测
pub fn name_check(str: &str) -> Result<&str, SysError> {
    let err = SysError::ENOENT;
    let str = name_trim(str);
    if str.is_empty() {
        return Err(err);
    }
    // utf8字节数最多为utf16的两倍 utf16最大空间为 31 * 13 * 2
    if str.len() > 31 * 13 * 2 * 2 {
        return Err(SysError::ENAMETOOLONG);
    }
    // utf8 check
    core::str::from_utf8(str.as_bytes()).map_err(|_| err)?;
    if str.bytes().any(|c| match c {
        c if !c.is_ascii() => false,
        c if c.is_ascii_control() => true,
        b'\\' | b'/' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|' => true,
        _ => false,
    }) {
        return Err(err);
    }
    Ok(str)
}
/// 字符串能只变为短文件名时返回Some
///
/// 方法:
pub fn str_to_just_short(str: &str) -> Option<([u8; 8], [u8; 3])> {
    debug_assert_eq!(str.len(), name_check(str).unwrap().len());
    if str.len() > 12 {
        return None;
    }
    let mut name = [0x20u8; 8];
    let mut ext = [0x20u8; 3];
    if str == "." {
        name[..1].copy_from_slice(".".as_bytes());
        return Some((name, ext));
    }
    if str == ".." {
        name[..2].copy_from_slice("..".as_bytes());
        return Some((name, ext));
    }
    if str.bytes().any(|c| match c {
        _ if !c.is_ascii() => true,
        b' ' | b'+' | b',' | b';' | b'=' | b'[' | b']' | b'a'..=b'z' => true,
        _ => false,
    }) {
        return None;
    }
    // now all chars are ascii
    let str = str.as_bytes();
    // find last '.'
    if let Some(i) = str
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, &c)| (c == b'.').then_some(i))
    {
        // 012345678.   0.1234   0.1.2
        //         ^         ^    ^
        let ext_len = str.len() - i;
        if i > 8 || ext_len > 3 || str[0..i].contains(&(b'.')) {
            return None;
        }
        name[..i].copy_from_slice(&str[..i]);
        ext[..ext_len].copy_from_slice(&str[i + 1..]);
        return Some((name, ext));
    }
    // 012345678
    //         ^
    if str.len() > 8 {
        return None;
    }
    name[..str.len()].copy_from_slice(str);
    Some((name, ext))
}
/// utf16顺序放置, 写入时应倒序遍历
///
/// 最大数组长度为31
pub fn str_to_utf16(str: &str) -> Result<Vec<[u16; 13]>, SysError> {
    debug_assert_eq!(str.len(), name_check(str).unwrap().len());
    const MAX_LEN: usize = 31;
    if str.is_empty() {
        return Ok(Vec::new());
    }
    let mut v = Vec::<[u16; 13]>::new();
    let mut i = 0;
    for ch in str.encode_utf16() {
        if i == 0 {
            if v.len() == MAX_LEN {
                return Err(SysError::ENAMETOOLONG);
            }
            v.push([0xFF; 13]);
        }
        v.last_mut().unwrap()[i] = ch;
        i += 1;
        if i >= 13 {
            i = 0;
        }
    }
    if i != 0 {
        v.last_mut().unwrap()[i] = 0x00;
    }
    Ok(v)
}

/// 小写字母变为大写字母 其他非法字符使用'_'代替
///
/// 后缀使用~+数字递增
///
/// 搜索策略: 匹配~~字符前的至多6byte 寻找 1位 1-9 都存在则哈希(2Bytes)+~4
pub(super) struct ShortFinder {
    name: [u8; 8],
    ext: [u8; 3],
    short_only: bool,
    name_len: usize,
    force: bool,          // 当为true时, 必须添加~x
    have_same: bool,      // 当为false且force为false时, 不需要添加~x
    num_mask: [bool; 10], // 不使用第0位
    hash: u16,
}

impl ShortFinder {
    pub fn new(src: &str) -> Self {
        let mut r: ShortFinder = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        if let Some((name, ext)) = str_to_just_short(src) {
            r.name = name;
            r.ext = ext;
            r.short_only = true;
            return r;
        }
        let mut have_invalid = false;
        let mut char_forward = |c: char| -> Option<u8> {
            if c == '_' {
                return Some(b'_');
            }
            let c = match c as u8 {
                _ if !c.is_ascii() => b'_',
                b'+' | b',' | b';' | b'=' | b'[' | b']' => b'_',
                b'.' => return None,
                c if c.is_ascii_lowercase() => c.to_ascii_uppercase(),
                _ => c as u8,
            };
            if c == b'_' {
                have_invalid = true;
            }
            Some(c)
        };
        // split by last '.'
        let dot = src
            .bytes()
            .enumerate()
            .rev()
            .find_map(|(i, c)| (c == b'.').then_some(i));
        // name
        let name_str = match dot {
            Some(i) => unsafe { core::str::from_utf8_unchecked(&src.as_bytes()[..i]) },
            None => src,
        };
        for c in name_str.chars().map(&mut char_forward).filter_map(|v| v) {
            if r.name_len == r.name.len() {
                r.force = true;
                break;
            }
            r.name[r.name_len] = c;
            r.name_len += 1;
        }
        r.name[r.name_len..].fill(0x20);
        // ext
        let ext_str = match dot {
            Some(i) => unsafe { core::str::from_utf8_unchecked(&src.as_bytes()[i + 1..]) },
            None => "",
        };
        let mut ext_len = 0;
        for c in ext_str.chars().map(&mut char_forward).filter_map(|v| v) {
            if ext_len == r.ext.len() {
                r.force = true;
                break;
            }
            r.ext[ext_len] = c;
            ext_len += 1;
        }
        r.ext[ext_len..].fill(0x20);
        r.force |= have_invalid;
        r.hash = Self::get_hash(src);
        r
    }
    fn get_hash(src: &str) -> u16 {
        const BASE: u16 = 5234;
        const M: u16 = 13719;
        const A: u16 = 9715;
        let mut v = BASE;
        for x in src.bytes() {
            v = v.wrapping_mul(M).wrapping_add(A) + x as u16;
        }
        v
    }
    /// 如果返回true则不存在长文件名, 短文件名冲突已经先前阶段检测, 一定不会重复
    pub fn short_only(&self) -> bool {
        self.short_only
    }
    pub fn record(&mut self, short: &Align8<RawShortName>) {
        if self.short_only || short.is_free() {
            return;
        }
        if self.ext != short.ext {
            return;
        }
        if self.name == short.name {
            self.have_same = true;
            return;
        }
        let check_p = self.name_len.min(6);
        if short
            .name
            .iter()
            .zip(self.name[..check_p].iter())
            .any(|(&c, &this)| c != this)
        {
            return;
        }
        if short.name[check_p] != b'~' {
            return;
        }
        let i = match short.name[check_p + 1] {
            c @ b'0'..=b'9' => c - b'0',
            _ => return,
        };
        if short
            .name
            .get(check_p + 2)
            .and_then(|&v| (v != 0x20).then_some(()))
            .is_some()
        {
            return;
        }
        self.num_mask[i as usize] = true;
    }
    pub fn apply(&self, dst: &mut Align8<RawShortName>) {
        dst.ext = self.ext;
        if self.short_only || (!self.force && !self.have_same) {
            dst.name = self.name;
            return;
        }
        // 寻找一个空闲的~x
        if let Some(p) = self
            .num_mask
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(i, &r)| (!r).then_some(i))
        {
            let sep = self.name_len.min(6);
            dst.name[..sep].copy_from_slice(&self.name[..sep]);
            dst.name[sep] = b'~';
            dst.name[sep + 1] = b'0' + p as u8;
            dst.name[sep + 2..].fill(0x20);
        }
        // 使用哈希大法
        fn to_ascii(n: u16) -> u8 {
            match n as u8 & 0xF {
                n @ 0..10 => b'0' + n,
                n @ 10..16 => b'A' + (n - 10),
                _ => unsafe { core::hint::unreachable_unchecked() },
            }
        }
        fn u16_ascii(n: u16, dst: &mut [u8; 4]) {
            for (i, c) in dst.iter_mut().rev().enumerate() {
                *c = to_ascii(n >> (i as u32) * 4);
            }
        }
        dst.name[0] = self.name[0];
        dst.name[1] = match self.name_len {
            0 => panic!(),
            1 => self.name[0],
            _ => self.name[1],
        };
        u16_ascii(self.hash, dst.name[2..].split_array_mut().0);
        dst.name[6] = b'~';
        dst.name[7] = b'4';
    }
}
