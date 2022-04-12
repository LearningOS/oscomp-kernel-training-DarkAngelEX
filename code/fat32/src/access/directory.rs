use core::{future::Future, ops::ControlFlow};

use alloc::{string::String, vec::Vec};

use crate::{
    block_cache::CacheRef,
    layout::{
        bpb::RawBPB,
        name::{Attr, Name, RawLongName, RawName, RawShortName},
    },
    manager::ManagerInner,
    mutex::SpinMutex,
    tools::{xasync::AsyncIter, Align8, CID},
    xerror::SysError,
    BlockDevice,
};

use super::common::Fat32Common;

pub struct Fat32Dir {
    common: Fat32Common,
}
impl Fat32Dir {
    pub fn new(cid: CID) -> Self {
        Self {
            common: Fat32Common::new(cid),
        }
    }
    /// 以只读模式访问块获取每一个目录项
    #[deprecated = "replace by raw_try_fold"]
    pub fn entry_iter<'a>(
        &'a mut self,
        mi: &'a SpinMutex<ManagerInner>,
        bpb: &'a RawBPB,
        device: &'a dyn BlockDevice,
    ) -> impl AsyncIter<Result<(RawName, (CID, usize)), SysError>> + 'a {
        let dirs_per_cluster = bpb.cluster_bytes / core::mem::size_of::<RawName>();
        return DirEntryIter {
            iter: self.common.cluster_iter(bpb, mi),
            offset: 0,
            dirs_per_cluster,
            cache: None,
            bpb,
            device,
        };

        struct DirEntryIter<'a, It: Iterator<Item = Result<CacheRef, SysError>>> {
            iter: It,
            offset: usize,
            dirs_per_cluster: usize,
            cache: Option<CacheRef>,
            bpb: &'a RawBPB,
            device: &'a dyn BlockDevice,
        }

        impl<'a, It> AsyncIter<Result<(RawName, (CID, usize)), SysError>> for DirEntryIter<'a, It>
        where
            It: Iterator<Item = Result<CacheRef, SysError>>,
        {
            type Item<'b>
            where
                Self: 'b,
            = impl Future<Output = Option<Result<(RawName, (CID, usize)), SysError>>> + 'b;
            fn next(&mut self) -> Self::Item<'_> {
                stack_trace!();
                let offset = self.offset;
                let cache = if offset == 0 {
                    self.iter.next()
                } else {
                    Some(Ok(self.cache.as_ref().unwrap().clone()))
                };
                if cache.is_some() {
                    self.offset += core::mem::size_of::<RawName>();
                    if self.offset >= self.dirs_per_cluster {
                        self.offset = 0;
                    }
                }
                let bpb = self.bpb;
                let device = self.device;
                async move {
                    let cache = cache?;
                    Some(
                        async move {
                            // Result<RawName, SysError>
                            let cache = cache?;
                            let mut raw_name = RawName::zeroed();
                            let op = |buf: &[RawName]| {
                                raw_name = buf[offset];
                            };
                            cache.get_ro(op, bpb, device).await?;
                            Ok((raw_name, (cache.cid(), offset)))
                        }
                        .await,
                    )
                }
            }
        }
    }

    pub async fn raw_try_fold<'a, B>(
        &'a mut self,
        mi: &'a SpinMutex<ManagerInner>,
        bpb: &'a RawBPB,
        device: &'a dyn BlockDevice,
        init: B,
        mut f: impl FnMut(B, &RawName, CID) -> ControlFlow<Result<B, SysError>, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        stack_trace!();
        let mut accum = init;
        let mut iter = self.common.cluster_iter(bpb, mi);
        while let Some(r) = iter.next() {
            let cache = match r {
                Ok(cache) => cache,
                Err(e) => return ControlFlow::Break(Err(e)),
            };
            let cid = cache.cid();
            let op = |buf: &[RawName]| buf.iter().try_fold(accum, |b, n| f(b, n, cid));
            accum = match cache.get_ro(op, bpb, device).await {
                Err(e) => return ControlFlow::Break(Err(e)),
                Ok(c) => c?,
            };
        }
        try { accum }
    }

    /// 此函数在迭代过程中将自动分配磁盘空间 新的空间将使每个项的首字节变成0x00
    pub async fn raw_try_fold_alloc<'a, B>(
        &'a mut self,
        mi: &'a SpinMutex<ManagerInner>,
        bpb: &'a RawBPB,
        device: &'a dyn BlockDevice,
        init: B,
        mut f: impl FnMut(B, &RawName, CID, usize) -> ControlFlow<Result<B, SysError>, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        stack_trace!();
        let mut accum = init;
        let mut iter = self
            .common
            .cluster_alloc_iter(bpb, mi, RawName::cluster_init);
        while let Some(r) = iter.next() {
            let cache = match r {
                Ok(cache) => cache,
                Err(e) => return ControlFlow::Break(Err(e)),
            };
            let cid = cache.cid();
            let op = |buf: &[RawName]| {
                buf.iter()
                    .enumerate()
                    .try_fold(accum, |b, (i, n)| f(b, n, cid, i))
            };
            accum = match cache.get_ro(op, bpb, device).await {
                Ok(r) => r?,
                Err(e) => return ControlFlow::Break(Err(e)),
            };
        }
        unreachable!()
    }
    /// 此函数在迭代过程中将自动分配磁盘空间 新的空间将使每个项的首字节变成0x00
    ///
    /// 控制流操作成功时将使用allpy函数对对应块进行操作 不释放睡眠锁
    pub async fn raw_try_fold_alloc_allpy<'a, B, A: 'static>(
        &'a mut self,
        mi: &'a SpinMutex<ManagerInner>,
        bpb: &'a RawBPB,
        device: &'a dyn BlockDevice,
        init: B,
        mut f: impl FnMut(B, &RawName, CID, usize) -> ControlFlow<Result<B, SysError>, B>,
        mut tran: impl FnMut(&ControlFlow<Result<B, SysError>, B>) -> Option<A>,
        mut apply: impl FnMut(A, &mut [RawName]),
    ) -> ControlFlow<Result<B, SysError>, B> {
        stack_trace!();
        let mut accum = init;
        let mut iter = self
            .common
            .cluster_alloc_iter(bpb, mi, RawName::cluster_init);
        while let Some(r) = iter.next() {
            let cache = match r {
                Ok(cache) => cache,
                Err(e) => ControlFlow::Break(Err(e))?,
            };
            let cid = cache.cid();
            let op = |buf: &[RawName]| {
                buf.iter()
                    .enumerate()
                    .try_fold(accum, |b, (i, n)| f(b, n, cid, i))
            };
            accum = match cache
                .get_apply(op, &mut tran, &mut apply, bpb, device)
                .await
            {
                Ok(r) => r?,
                Err(e) => ControlFlow::Break(Err(e))?,
            };
        }
        // 此迭代器永远不会返回None
        unreachable!()
    }
    /// 以只读模式遍历每一个文件项 自动加载长文件名并转变编码为utf-8 String
    #[deprecated = "replace by name_try_fold"]
    pub fn name_iter<'a>(
        &'a mut self,
        mi: &'a SpinMutex<ManagerInner>,
        bpb: &'a RawBPB,
        device: &'a dyn BlockDevice,
    ) -> impl AsyncIter<Result<DirName, SysError>> + 'a {
        #[allow(deprecated)]
        let iter = self.entry_iter(mi, bpb, device);
        return NameIter { iter };

        struct NameIter<It: AsyncIter<Result<(RawName, (CID, usize)), SysError>>> {
            iter: It,
        }

        impl<It> AsyncIter<Result<DirName, SysError>> for NameIter<It>
        where
            It: AsyncIter<Result<(RawName, (CID, usize)), SysError>>,
        {
            type Item<'a>
            where
                Self: 'a,
            = impl Future<Output = Option<Result<DirName, SysError>>> + 'a;

            fn next(&mut self) -> Self::Item<'_> {
                async {
                    let mut long_name = LongNameBuilder::new();
                    while let Some(a) = self.iter.next().await {
                        let (raw, _) = match a {
                            Ok(raw) => raw,
                            Err(e) => return Some(Err(e)),
                        };
                        let name = match raw.get() {
                            Some(name) => name,
                            None => {
                                long_name.clear();
                                continue;
                            }
                        };
                        match name {
                            Name::Short(s) => {
                                let dir_name = DirName::build_new(&long_name, s);
                                return Some(Ok(dir_name));
                            }
                            Name::Long(l) => {
                                long_name.push_long(l);
                            }
                        }
                    }
                    None
                }
            }
        }
    }
    pub async fn name_try_fold<B>(
        &mut self,
        mi: &SpinMutex<ManagerInner>,
        bpb: &RawBPB,
        device: &dyn BlockDevice,
        init: B,
        mut f: impl FnMut(B, DirName) -> ControlFlow<Result<B, SysError>, B>,
    ) -> ControlFlow<Result<B, SysError>, B> {
        stack_trace!();
        let mut builder = LongNameBuilder::new();
        let ret = self
            .raw_try_fold(
                mi,
                bpb,
                device,
                (init, &mut builder),
                |(accum, builder), raw, _cid| match raw.get() {
                    None => {
                        builder.clear();
                        try { (accum, builder) }
                    }
                    Some(Name::Long(long)) => {
                        builder.push_long(long);
                        try { (accum, builder) }
                    }
                    Some(Name::Short(s)) => match f(accum, DirName::build_new(&builder, s)) {
                        ControlFlow::Continue(a) => {
                            builder.clear();
                            try { (a, builder) }
                        }
                        ControlFlow::Break(Ok(a)) => ControlFlow::Break(Ok((a, builder))),
                        ControlFlow::Break(Err(e)) => ControlFlow::Break(Err(e)),
                    },
                },
            )
            .await;
        match ret {
            ControlFlow::Continue((b, _)) => ControlFlow::Continue(b),
            ControlFlow::Break(r) => ControlFlow::Break(r.map(|(b, _)| b)),
        }
    }

    pub async fn insert_file(
        &mut self,
        long: &str,
        short: &RawShortName,
        mi: &SpinMutex<ManagerInner>,
        bpb: &RawBPB,
        device: &dyn BlockDevice,
    ) -> Result<(), SysError> {
        stack_trace!();
        // 寻找连续的簇
        let long = str_to_utf16(long);
        let num = long.len() + 1;
        let num_cluster = bpb.cluster_bytes / core::mem::size_of::<RawName>();
        let checksum = short.checksum();
        #[derive(Clone, Copy)]
        struct State {
            cid: CID,   // 开始项簇号
            off: usize, // 开始项偏移量
            cnt: usize, // 此项是第几个空项
        }
        let mut init = State {
            cid: CID(0),
            off: 0,
            cnt: 0,
        };

        let ret = self
            .raw_try_fold_alloc_allpy(
                mi,
                bpb,
                device,
                &mut init,
                // 找到一个簇内的连续空位
                |s, raw, cid, off| {
                    if !raw.is_free() || off + num > num_cluster {
                        s.cnt = 0;
                        return ControlFlow::Continue(s);
                    }
                    s.cnt += 1;
                    if s.cnt == 1 {
                        s.cid = cid;
                        s.off = off;
                    }
                    if s.cnt == num {
                        return ControlFlow::Break(Ok(s));
                    }
                    ControlFlow::Continue(s)
                },
                // control_flow -> state
                |a| match a {
                    ControlFlow::Break(Ok(s)) => Some(**s),
                    _ => None,
                },
                // 写入
                move |a: State, buf| {
                    let long_num = num - 1;
                    for (i, (dst, src)) in buf[a.off..a.off + long_num]
                        .iter_mut()
                        .rev()
                        .zip(long.iter())
                        .enumerate()
                    {
                        dst.set_long(src, i + 1, i + 1 == long_num, checksum);
                    }
                    buf[num - 1].set_short(short);
                },
            )
            .await;
        let &mut _state = match ret {
            ControlFlow::Continue(_) => unreachable!(),
            ControlFlow::Break(r) => r?,
        };
        Ok(())
    }
    // long为正常顺序
    pub async fn search_file(
        &mut self,
        file_name: &str,
        mi: &SpinMutex<ManagerInner>,
        bpb: &RawBPB,
        device: &dyn BlockDevice,
    ) -> Result<Option<Align8<RawShortName>>, SysError> {
        macro_rules! do_impl {
            ($match_fn: expr) => {{
                let f = move |_, name| {
                    if $match_fn(&name) {
                        ControlFlow::Break(Ok(Some(name.short)))?;
                    }
                    try { None }
                };
                match self.name_try_fold(mi, bpb, device, None, f).await {
                    ControlFlow::Continue(_) => Ok(None),
                    ControlFlow::Break(b) => b,
                }
            }};
        }
        if let Some(short) = str_to_just_short(file_name) {
            let match_fn = |name: &DirName| name.short.raw_name() == short;
            do_impl!(match_fn)
        } else {
            let match_fn = |name: &DirName| name.long.as_str() == file_name;
            do_impl!(match_fn)
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
/// 只有字符串能只变为短文件名时返回Some
fn str_to_just_short(_src: &str) -> Option<([u8; 8], [u8; 3])> {
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

pub struct DirName {
    pub long: String,
    pub short: Align8<RawShortName>,
}

impl DirName {
    fn build_new(builder: &LongNameBuilder, short: &Align8<RawShortName>) -> Self {
        Self {
            long: builder.decode_utf16(),
            short: *short,
        }
    }
    fn attribute(&self) -> Attr {
        self.short.attributes
    }
    fn cid(&self) -> CID {
        let h16 = (self.short.cluster_h16 as u32) << 16;
        CID(h16 | self.short.cluster_l16 as u32)
    }
}
