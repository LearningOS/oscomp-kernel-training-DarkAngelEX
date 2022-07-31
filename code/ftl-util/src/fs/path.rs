use alloc::vec::Vec;

use crate::error::SysR;

pub fn walk_iter_path<'a>(
    base: impl FnOnce<(), Output = SysR<impl Iterator<Item = &'a str>>>,
    path: &'a str,
) -> SysR<Vec<&'a str>> {
    let mut v = Vec::new();
    if !is_absolute_path(path) {
        for s in base()? {
            v.push(s)
        }
    }
    for s in path.split(['/', '\\']).map(|s| s.trim()) {
        match s {
            "" | "." => continue,
            ".." => {
                v.pop();
            }
            s => {
                v.push(s);
            }
        }
    }
    Ok(v)
}

pub fn walk_path_impl<'a>(path: &'a str, v: &mut Vec<&'a str>) {
    for s in path.split(['/', '\\']).map(|s| s.trim()) {
        match s {
            "" | "." => continue,
            ".." => {
                v.pop();
            }
            s => {
                v.push(s);
            }
        }
    }
}

pub fn write_path_to<'a>(src: impl Iterator<Item = &'a str>, dst: &mut [u8]) {
    assert!(dst.len() >= 2);
    let max = dst.len() - 1;
    dst[0] = b'/';
    dst[max] = b'\0';
    let mut p = 0;
    for s in src {
        assert!(p + 1 + s.len() <= max);
        dst[p] = b'/';
        p += 1;
        dst[p..p + s.len()].copy_from_slice(s.as_bytes());
        p += s.len();
    }
    dst[p] = b'\0';
}

pub fn is_absolute_path(s: &str) -> bool {
    matches!(s.as_bytes().first(), Some(b'/') | Some(b'\\'))
}
