use ftl_util::error::{SysError, SysR};

use crate::{dentry::Dentry, hash_name::HashName, VfsManager};

use super::BaseFn;

impl VfsManager {
    pub fn walk_path<'a>(&self, (base, path): (impl BaseFn, &str)) -> SysR<Dentry> {
        let mut dentry = if !is_absolute_path(path) {
            base()?
        } else {
            self.root.clone()
        };
        for s in path.split(['/', '\\']).map(|s| s.trim()) {
            match s {
                "" | "." => continue,
                ".." => {
                    if let Some(p) = dentry.parent() {
                        dentry = p
                    }
                    continue;
                }
                s if name_invalid(s) => return Err(SysError::ENOENT),
                s => {
                    let name_hash = HashName::hash_name(s);
                    // if let Some(c) = dentry.try_child(s, name_hash)? {
                    //     dentry = c;
                    //     continue;
                    // }
                    todo!()
                }
            }
        }
        // Ok(v)
        todo!()
    }
}

fn name_invalid(s: &str) -> bool {
    s.bytes().any(|c| match c {
        b'\\' | b'/' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|' => true,
        _ => false,
    })
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
    match s.as_bytes().first() {
        Some(b'/') | Some(b'\\') => true,
        _ => false,
    }
}
