use alloc::vec::Vec;

pub fn walk_iter_path<'a>(src: impl Iterator<Item = &'a str>, dst: &mut Vec<&'a str>) {
    for s in src {
        dst.push(s)
    }
}

pub fn walk_path<'a>(src: &'a str, dst: &mut Vec<&'a str>) {
    for s in src.split(['/', '\\']).map(|s| s.trim()) {
        match s {
            "" | "." => continue,
            ".." => {
                dst.pop();
            }
            s => {
                dst.push(s);
            }
        }
    }
}
