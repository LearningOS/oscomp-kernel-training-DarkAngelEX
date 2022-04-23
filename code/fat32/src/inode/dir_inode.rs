use core::ops::ControlFlow;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    layout::name::{Attr, Name, RawLongName, RawName, RawShortName},
    mutex::rw_sleep_mutex::RwSleepMutex,
    tools::{Align8, CID},
    xerror::SysError,
    Fat32Manager,
};

use super::raw_inode::RawInode;

#[derive(Debug, Clone, Copy)]
pub struct EntryPlace {
    cluster_off: usize, // inode簇偏移
    cid: CID,
    entry_off: usize,
}

impl EntryPlace {
    pub fn new(cluster_off: usize, cid: CID, entry_off: usize) -> Self {
        Self {
            cluster_off,
            cid,
            entry_off,
        }
    }
}

/// 不要在这里维护任何数据 数据都放在inode中
pub struct DirInode {
    inode: Arc<RwSleepMutex<RawInode>>, // 只有需要改变文件大小时才需要排他锁
}

impl DirInode {
    pub fn new(inode: Arc<RwSleepMutex<RawInode>>) -> Self {
        Self { inode }
    }
    // unique
    pub async fn create_dir(&self, manager: &Fat32Manager) -> Result<(), SysError> {
        todo!()
    }
    // unique
    pub async fn create_file(&self, manager: &Fat32Manager) -> Result<(), SysError> {
        todo!()
    }
    // shared
    pub async fn delete(&self, manager: &Fat32Manager) -> Result<(), SysError> {
        todo!()
    }
    pub async fn create_entry(
        &self,
        manager: &Fat32Manager,
        name: &str,
        short: Align8<RawShortName>,
    ) -> Result<(), SysError> {
        let mut inode = self.inode.unique_lock().await;
        if Self::search_entry(&*inode, manager, name).await?.is_some() {
            return Err(SysError::EEXIST);
        }
        let long = if str_to_just_short(name).is_some() {
            None
        } else {
            Some(str_to_utf16(name))
        };
        let need_len = long.as_ref().map(|a| a.len() + 1).unwrap_or(1);
        let r = Self::raw_entry_try_fold(&*inode, manager, (0, None), |(cnt, place), b, c| {
            if !b.is_free() {
                return ControlFlow::Continue((0, None));
            }
            let nxt_place = if cnt == 0 { Some(c) } else { place };
            let nxt = (cnt + 1, nxt_place);
            if nxt.0 == need_len {
                ControlFlow::Break(nxt)
            } else {
                ControlFlow::Continue(nxt)
            }
        })
        .await;
        let (n, p) = match r {
            ControlFlow::Break(Err(e)) => return Err(e),
            ControlFlow::Continue(x) | ControlFlow::Break(Ok(x)) => x,
        };
        if n != need_len {
            let (cid, cache) = inode.append_block(manager, RawName::cluster_init).await?;
            todo!()
        }
        todo!()
    }
    /// 返回短文件名位置
    ///
    /// (簇偏移, 簇内偏移, 簇号)
    async fn search_entry(
        inode: &RawInode,
        manager: &Fat32Manager,
        name: &str,
    ) -> Result<Option<(EntryPlace, Align8<RawShortName>)>, SysError> {
        if let Some(short) = &str_to_just_short(name) {
            match Self::name_try_fold(inode, manager, None, |_prev, b| {
                if b.short_cmp(short) {
                    return ControlFlow::Break(Some((b.place(), b.short)));
                }
                try { None }
            })
            .await
            {
                ControlFlow::Continue(c) => return Ok(c),
                ControlFlow::Break(b) => return b,
            }
        }
        match Self::name_try_fold(inode, manager, None, |_prev, b| {
            if b.long_cmp(name) {
                return ControlFlow::Break(Some((b.place(), b.short)));
            }
            try { None }
        })
        .await
        {
            ControlFlow::Continue(c) => Ok(c),
            ControlFlow::Break(b) => b,
        }
    }
    async fn raw_entry_try_fold<B>(
        inode: &RawInode,
        manager: &Fat32Manager,
        init: B,
        mut f: impl FnMut(B, &RawName, EntryPlace) -> ControlFlow<B, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        stack_trace!();
        let mut accum = init;
        let mut block_off = 0;
        loop {
            let r = match inode.get_nth_block(manager, block_off).await {
                Ok(r) => r,
                Err(e) => return ControlFlow::Break(Err(e)),
            };
            let (cid, cache) = match r {
                Ok(cache) => cache,
                Err(_) => {
                    return try { accum };
                }
            };
            accum = cache
                .access_ro(|a| {
                    match a.iter().try_fold((accum, 0), |(b, off), raw| {
                        let place = EntryPlace::new(block_off, cid, off);
                        match f(b, raw, place) {
                            ControlFlow::Continue(b) => try { (b, off + 1) },
                            ControlFlow::Break(b) => ControlFlow::Break((b, off + 1)),
                        }
                    }) {
                        ControlFlow::Continue((b, _o)) => try { b },
                        ControlFlow::Break((b, _o)) => ControlFlow::Break(b),
                    }
                })
                .await
                .map_break(|b| Ok(b))?;
            block_off += 1;
        }
    }
    async fn name_try_fold<B>(
        inode: &RawInode,
        manager: &Fat32Manager,
        init: B,
        mut f: impl FnMut(B, DirName) -> ControlFlow<B, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        match Self::raw_entry_try_fold(
            inode,
            manager,
            (init, &mut LongNameBuilder::new()),
            |(accum, builder), raw, place| match raw.get() {
                None => {
                    builder.clear();
                    try { (accum, builder) }
                }
                Some(Name::Long(long)) => {
                    builder.push_long(long);
                    try { (accum, builder) }
                }
                Some(Name::Short(s)) => match f(accum, DirName::build_new(builder, s, place)) {
                    ControlFlow::Continue(accum) => {
                        builder.clear();
                        try { (accum, builder) }
                    }
                    ControlFlow::Break(b) => ControlFlow::Break((b, builder)),
                },
            },
        )
        .await
        {
            ControlFlow::Continue((b, _lb)) => try { b },
            ControlFlow::Break(b) => ControlFlow::Break(b.map(|(b, _lb)| b)),
        }
    }
}

struct LongNameBuilder {
    long: Vec<[u16; 13]>,
    current: usize,
}

impl LongNameBuilder {
    const fn new() -> Self {
        Self {
            long: Vec::new(),
            current: 0,
        }
    }
    fn clear(&mut self) {
        self.long.clear();
    }
    fn push_long(&mut self, s: &RawLongName) {
        if s.is_last() {
            self.current = s.order_num();
        } else if self.current != s.order_num() + 1 {
            self.current = 0;
        }
        if self.current == 0 {
            self.long.clear();
            return;
        }
        self.current = s.order_num();
        self.long.push([0; 13]);
        s.store_name(self.long.last_mut().unwrap());
    }
    fn decode_utf16(&self) -> String {
        if self.current != 1 {
            return String::new();
        }
        utf16_to_string(self.long.iter())
    }
}

/// 长文件名反序 最后一项在前
fn utf16_to_string<'a>(src: impl DoubleEndedIterator<Item = &'a [u16; 13]>) -> String {
    let u16_iter = src
        .rev()
        .flat_map(|&s| s.into_iter())
        .take_while(|&s| s != 0x00)
        .into_iter();
    char::decode_utf16(u16_iter)
        .map(|r| r.unwrap_or(core::char::REPLACEMENT_CHARACTER))
        .collect()
}
/// 字符串能只变为短文件名时返回Some
fn str_to_just_short(src: &str) -> Option<([u8; 8], [u8; 3])> {
    if src.len() >= 12 {
        return None;
    }
    todo!()
}
/// 正常顺序
fn str_to_utf16(src: &str) -> Vec<[u16; 13]> {
    if src.is_empty() {
        return Vec::new();
    }
    let mut v = Vec::<[u16; 13]>::new();
    let mut i = 0;
    for ch in src.encode_utf16() {
        if i == 0 {
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
    v
}

struct DirName {
    pub long: String,
    pub short: Align8<RawShortName>,
    pub place: EntryPlace,
}

impl DirName {
    fn build_new(
        builder: &LongNameBuilder,
        short: &Align8<RawShortName>,
        place: EntryPlace,
    ) -> Self {
        Self {
            long: builder.decode_utf16(),
            short: *short,
            place,
        }
    }
    fn attribute(&self) -> Attr {
        self.short.attributes
    }
    fn cid(&self) -> CID {
        let h16 = (self.short.cluster_h16 as u32) << 16;
        CID(h16 | self.short.cluster_l16 as u32)
    }
    fn place(&self) -> EntryPlace {
        self.place
    }
    fn short_cmp(&self, short: &([u8; 8], [u8; 3])) -> bool {
        &(self.short.name, self.short.ext) == short
    }
    fn long_cmp(&self, str: &str) -> bool {
        self.long == str
    }
}
