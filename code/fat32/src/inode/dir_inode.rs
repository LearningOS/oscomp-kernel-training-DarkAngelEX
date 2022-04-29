use core::{convert::Infallible, ops::ControlFlow};

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    layout::name::{Attr, Name, RawLongName, RawName, RawShortName},
    mutex::rw_sleep_mutex::RwSleepMutex,
    tools::{Align8, CID},
    xerror::SysError,
    Fat32Manager, FileInode,
};

use super::{
    inode_cache::InodeCache,
    raw_inode::RawInode,
    xstr::{name_check, str_to_just_short, str_to_utf16, utf16_to_string, ShortFinder},
    IID,
};

/// 短目录项偏移
///
/// 根目录为全0
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntryPlace {
    pub cluster_off: usize, // inode簇偏移
    pub cid: CID,           // 文件项所在簇的CID
    pub entry_off: usize,   // 文件项的簇内偏移
}

impl EntryPlace {
    pub const fn new(cluster_off: usize, cid: CID, entry_off: usize) -> Self {
        Self {
            cluster_off,
            cid,
            entry_off,
        }
    }
    pub const ROOT: Self = Self::new(0, CID::FREE, 0);
    #[inline(always)]
    pub fn iid(&self, manager: &Fat32Manager) -> IID {
        IID::new(self.cid, self.entry_off, manager.bpb.cluster_bytes_log2)
    }
}

/// 不要在这里维护任何数据 数据都放在inode中
#[derive(Clone)]
pub struct DirInode {
    inode: Arc<RwSleepMutex<RawInode>>, // 只有需要改变文件大小时才需要排他锁
}

impl DirInode {
    pub(crate) fn new(inode: Arc<RwSleepMutex<RawInode>>) -> Self {
        Self { inode }
    }
    pub async fn list(&self, manager: &Fat32Manager) -> Result<Vec<String>, SysError> {
        let inode = &*self.inode.shared_lock().await;
        let mut set = Vec::new();
        Self::name_try_fold(inode, manager, (), |(), dir| {
            set.push(dir.take_name());
            ControlFlow::<Infallible>::Continue(())
        })
        .await?;
        Ok(set)
    }
    pub async fn search_dir(
        &self,
        manager: &Fat32Manager,
        name: &str,
    ) -> Result<DirInode, SysError> {
        let cache = self
            .raw_search(manager, name)
            .await?
            .ok_or(SysError::ENOENT)?;
        if !cache.inner.shared_lock().attr().contains(Attr::DIRECTORY) {
            return Err(SysError::ENOTDIR);
        }
        Ok(DirInode::new(cache.get_inode(unsafe {
            self.inode.unsafe_get().cache.clone()
        })))
    }
    pub async fn search_file(
        &self,
        manager: &Fat32Manager,
        name: &str,
    ) -> Result<FileInode, SysError> {
        let cache = self
            .raw_search(manager, name)
            .await?
            .ok_or(SysError::ENOENT)?;
        if cache.inner.shared_lock().attr().contains(Attr::DIRECTORY) {
            return Err(SysError::ENOTDIR);
        }
        Ok(FileInode::new(cache.get_inode(unsafe {
            self.inode.unsafe_get().cache.clone()
        })))
    }
    async fn raw_search(
        &self,
        manager: &Fat32Manager,
        name: &str,
    ) -> Result<Option<Arc<InodeCache>>, SysError> {
        stack_trace!();
        let name = name_check(name)?;
        let inode = &*self.inode.shared_lock().await;
        let (short, (_, place)) = match Self::search_impl(inode, manager, name).await? {
            None => return Ok(None),
            Some(x) => x,
        };
        let iid = place.iid(manager);
        Ok(Some(manager.inodes.get_or_insert(iid, || {
            InodeCache::from_parent(manager, short, place, inode)
        })))
    }
    pub async fn create_dir(
        &self,
        manager: &Fat32Manager,
        name: &str,
        read_only: bool,
        hidden: bool,
    ) -> Result<(), SysError> {
        stack_trace!();
        let name = name_check(name)?;
        // 排他锁保证文件分配不被打乱
        let inode = &mut *self.inode.unique_lock().await;
        if Self::search_impl(inode, manager, name).await?.is_some() {
            return Err(SysError::EEXIST);
        }
        // 寻找短文件名
        let finder = Self::short_detect(inode, manager, name).await?;
        let utc_time = manager.utc_time();
        let parent_cid = inode.parent.inner.shared_lock().cid_start;
        let this_cid = inode.cache.inner.shared_lock().cid_start;
        debug_assert!(parent_cid.is_next());
        debug_assert!(this_cid.is_next());
        // 为目录分配新的簇
        let cid = manager.list.alloc_block().await?;
        debug_assert!(cid.is_next());
        manager
            .caches
            .get_block_init(cid, |a| {
                RawName::cluster_init(a);
                a[0].short_init().init_dot_dir(2, parent_cid, &utc_time);
                a[1].short_init().init_dot_dir(1, this_cid, &utc_time);
            })
            .await?;
        let mut short = Align8(RawShortName::zeroed());
        finder.apply(&mut short);
        short.init_except_name(cid, 0, Attr::new(true, read_only, hidden), &utc_time);
        Self::create_entry_impl(inode, manager, name, short).await?;
        Ok(())
    }
    /// 创建一个空文件
    pub async fn create_file(
        &self,
        manager: &Fat32Manager,
        name: &str,
        read_only: bool,
        hidden: bool,
    ) -> Result<(), SysError> {
        stack_trace!();
        let name = name_check(name)?;
        // 排他锁保证文件分配不被打乱
        let inode = &mut *self.inode.unique_lock().await;
        if Self::search_impl(inode, manager, name).await?.is_some() {
            return Err(SysError::EEXIST);
        }
        // 寻找短文件名
        let finder = Self::short_detect(inode, manager, name).await?;
        let utc_time = manager.utc_time();
        let mut short = Align8(RawShortName::zeroed());
        finder.apply(&mut short);
        short.init_except_name(CID::FREE, 0, Attr::new(false, read_only, hidden), &utc_time);
        Self::create_entry_impl(inode, manager, name, short).await?;
        Ok(())
    }
    /// 寻找可用的短文件名
    async fn short_detect(
        inode: &mut RawInode,
        manager: &Fat32Manager,
        name: &str,
    ) -> Result<ShortFinder, SysError> {
        // 寻找短文件名
        let mut finder = ShortFinder::new(name);
        if !finder.short_only() {
            Self::raw_entry_try_fold(inode, manager, (), |(), name, _e| {
                if let Some(Name::Short(short)) = name.get() {
                    finder.record(short)
                }
                ControlFlow::<Infallible>::CONTINUE
            })
            .await?;
        }
        Ok(finder)
    }
    /// 自动判断是删除目录还是文件
    pub async fn delete_any(&self, manager: &Fat32Manager, name: &str) -> Result<(), SysError> {
        stack_trace!();
        let name = name_check(name)?;
        let mut inode = self.inode.unique_lock().await;
        let (short, place) = match Self::search_impl(&*inode, manager, name).await? {
            Some(x) => x,
            None => return Err(SysError::ENOENT),
        };
        let cid = match short.is_dir() {
            true => Self::delete_dir_impl(&mut *inode, manager, short, place).await?,
            false => Self::delete_file_impl(&mut *inode, manager, short, place).await?,
        };
        drop(inode);
        if cid.is_next() {
            manager.list.free_cluster_at(cid).await.1?;
            manager.list.free_cluster(cid).await?;
        }
        Ok(())
    }
    /// 目录必须为空 只删除仅含有 ".." "." 的目录
    pub async fn delete_dir(&self, manager: &Fat32Manager, name: &str) -> Result<(), SysError> {
        stack_trace!();
        let name = name_check(name)?;
        let mut inode = self.inode.unique_lock().await;
        let (short, place) = match Self::search_impl(&*inode, manager, name).await? {
            Some(x) => x,
            None => return Err(SysError::ENOENT),
        };
        if !short.is_dir() {
            return Err(SysError::ENOTDIR);
        }
        let cid = Self::delete_dir_impl(&mut *inode, manager, short, place).await?;
        drop(inode);
        if cid.is_next() {
            manager.list.free_cluster_at(cid).await.1?;
            manager.list.free_cluster(cid).await?;
        }
        Ok(())
    }
    /// shared
    pub async fn delete_file(&self, manager: &Fat32Manager, name: &str) -> Result<(), SysError> {
        stack_trace!();
        let name = name_check(name)?;
        let mut inode = self.inode.unique_lock().await;
        let (short, place) = match Self::search_impl(&*inode, manager, name).await? {
            Some(x) => x,
            None => return Err(SysError::ENOENT),
        };
        if short.is_dir() {
            return Err(SysError::EISDIR);
        }
        let cid = Self::delete_file_impl(&mut *inode, manager, short, place).await?;
        drop(inode);
        // release list
        if cid.is_next() {
            manager.list.free_cluster_at(cid).await.1?;
            manager.list.free_cluster(cid).await?;
        }
        Ok(())
    }
    async fn delete_dir_impl(
        inode: &mut RawInode,
        manager: &Fat32Manager,
        short: Align8<RawShortName>,
        place: (EntryPlace, EntryPlace),
    ) -> Result<CID, SysError> {
        // 文件如果处于打开状态将返回Err
        manager.inodes.unused_release(place.1.iid(manager))?;
        // 这里将持有唯一打开的引用
        let dir = manager
            .inodes
            .get_or_insert(place.1.iid(manager), || {
                InodeCache::from_parent(manager, short, place.1, inode)
            })
            .get_inode(inode.cache.clone());
        // 检查是否是空文件夹
        let r = Self::name_try_fold(&*dir.shared_lock().await, manager, (), |(), b| {
            match b.is_dot() {
                true => ControlFlow::CONTINUE,
                false => ControlFlow::BREAK,
            }
        })
        .await?;
        if r.is_break() {
            return Err(SysError::ENOTEMPTY);
        }
        manager.inodes.unused_release(place.1.iid(manager))?;
        let cid = Self::delete_entry(&mut *inode, manager, place).await?;
        debug_assert!(cid == short.cid());
        Ok(cid)
    }
    async fn delete_file_impl(
        inode: &mut RawInode,
        manager: &Fat32Manager,
        short: Align8<RawShortName>,
        place: (EntryPlace, EntryPlace),
    ) -> Result<CID, SysError> {
        manager.inodes.unused_release(place.1.iid(manager))?;
        let cid = Self::delete_entry(&mut *inode, manager, place).await?;
        debug_assert!(cid == short.cid());
        Ok(cid)
    }
    // =============================================================================
    // ============ private ============  private  ============ private ============
    // =============================================================================

    /// 删除entry项 返回FAT链表
    ///
    /// 删除 start_place -> short_place 的长文件名和短文件名
    async fn delete_entry(
        _inode: &mut RawInode, // 存粹数据修改不需要改动inode, 但依然要获取排他锁
        manager: &Fat32Manager,
        (start_place, short_place): (EntryPlace, EntryPlace), // 长文件名起始项 如果没有长文件名则等于short_place
    ) -> Result<CID, SysError> {
        stack_trace!();
        // 检测是否链表为空
        if cfg!(debug_assert) {
            let cache = manager.caches.get_block(short_place.cid).await?;
            cache
                .access_ro(
                    |names: &[RawName]| match names[short_place.entry_off].get().unwrap() {
                        Name::Long(_) => panic!(),
                        Name::Short(s) => assert_eq!(s.cid(), CID::FREE),
                    },
                )
                .await;
        }
        let single = start_place.cluster_off == short_place.cluster_off;
        // 删除前一个簇的文件名
        let cid = manager
            .caches
            .write_block(
                start_place.cid,
                &*manager.caches.get_block(start_place.cid).await?,
                |x: &mut [RawName]| {
                    // 删除长文件名
                    if single {
                        &mut x[start_place.entry_off..short_place.entry_off]
                    } else {
                        &mut x[start_place.entry_off..]
                    }
                    .iter_mut()
                    .for_each(|dst| {
                        debug_assert!(dst.is_long());
                        dst.set_free();
                    });
                    // 删除短文件名
                    if single {
                        let short = &mut x[short_place.entry_off];
                        let cid = short.get_short().unwrap().cid();
                        short.set_free();
                        Some(cid)
                    } else {
                        None
                    }
                },
            )
            .await?;
        if let Some(cid) = cid {
            debug_assert!(single);
            return Ok(cid);
        }
        debug_assert!(!single);
        // 删除后一个簇的文件名
        let cid = manager
            .caches
            .write_block(
                short_place.cid,
                &*manager.caches.get_block(short_place.cid).await?,
                |x: &mut [RawName]| {
                    // 删除长文件名
                    x[..short_place.entry_off].iter_mut().for_each(|dst| {
                        debug_assert!(dst.is_long());
                        dst.set_free();
                    });
                    // 删除短文件名
                    let short = &mut x[short_place.entry_off];
                    let cid = short.get_short().unwrap().cid();
                    short.set_free();
                    cid
                },
            )
            .await?;
        Ok(cid)
    }
    /// 返回短文件名的位置
    async fn create_entry_impl(
        inode: &mut RawInode,
        manager: &Fat32Manager,
        name: &str,
        short: Align8<RawShortName>,
    ) -> Result<EntryPlace, SysError> {
        stack_trace!();
        if Self::search_impl(inode, manager, name).await?.is_some() {
            return Err(SysError::EEXIST);
        }
        let long = if str_to_just_short(name).is_some() {
            Vec::new()
        } else {
            str_to_utf16(name)?
        };
        let need_len = long.len() + 1;
        // (连续空位数, 第一个空entry的位置)
        let r = Self::raw_entry_try_fold(inode, manager, (0, None), |(cnt, place), b, c| {
            if !b.is_free() {
                return ControlFlow::Continue((0, Some(c)));
            }
            let nxt_place = if cnt == 0 { Some(c) } else { place };
            let nxt = (cnt + 1, nxt_place);
            if nxt.0 == need_len {
                ControlFlow::Break(nxt)
            } else {
                ControlFlow::Continue(nxt)
            }
        })
        .await?;
        // 连续空闲数 位置
        let (n, p) = match r {
            ControlFlow::Continue(x) | ControlFlow::Break(x) => x,
        };
        let mut iter = entry_generate(&long, &short);
        // 在新的块从头开始写入
        if p.is_none() || n == 0 {
            let (cluster_off, cid, cache) =
                inode.append_block(manager, RawName::cluster_init).await?;
            manager
                .caches
                .write_block(cid, &cache, |a: &mut [RawName]| {
                    a.iter_mut().zip(iter).for_each(|(dst, src)| {
                        debug_assert!(dst.is_free());
                        *dst = src;
                    });
                })
                .await?;
            return Ok(EntryPlace::new(cluster_off, cid, need_len - 1));
        }
        let p = p.unwrap();
        // 只在新的块写入
        manager
            .caches
            .write_block(
                p.cid,
                &*manager.caches.get_block(p.cid).await?,
                |a: &mut [RawName]| {
                    a[p.entry_off..]
                        .iter_mut()
                        .zip(&mut iter)
                        .for_each(|(dst, src)| {
                            debug_assert!(dst.is_free());
                            *dst = src;
                        });
                },
            )
            .await?;
        if n == need_len {
            return Ok(EntryPlace::new(
                p.cluster_off,
                p.cid,
                p.entry_off + need_len - 1,
            ));
        }
        // 同时在当前块和新的块写入
        let (cluster_off, cid_2, b_2) = inode.append_block(manager, RawName::cluster_init).await?;
        let mut entry_off = 0;
        manager
            .caches
            .write_block(cid_2, &b_2, |a: &mut [RawName]| {
                iter.zip(a.iter_mut()).for_each(|(src, dst)| {
                    debug_assert!(dst.is_free());
                    entry_off += 1;
                    *dst = src;
                });
            })
            .await?;
        return Ok(EntryPlace::new(cluster_off, cid_2, entry_off - 1));
    }
    /// 返回短文件名 文件名首项位置 短文件名位置
    async fn search_impl(
        inode: &RawInode,
        manager: &Fat32Manager,
        name: &str,
    ) -> Result<Option<(Align8<RawShortName>, (EntryPlace, EntryPlace))>, SysError> {
        stack_trace!();
        if let Some(short) = &str_to_just_short(name) {
            let r = Self::name_try_fold(inode, manager, (), |(), b| {
                if b.short_same(short) {
                    return ControlFlow::Break((b.short, b.place()));
                }
                try { () }
            })
            .await?;
            return match r {
                ControlFlow::Continue(()) => Ok(None),
                ControlFlow::Break(b) => Ok(Some(b)),
            };
        }
        let r = Self::name_try_fold(inode, manager, (), |(), b| {
            if b.long_same(name) {
                return ControlFlow::Break((b.short, b.place()));
            }
            try { () }
        })
        .await?;
        match r {
            ControlFlow::Continue(()) => Ok(None),
            ControlFlow::Break(b) => return Ok(Some(b)),
        }
    }
    async fn raw_entry_try_fold<A, B>(
        inode: &RawInode,
        manager: &Fat32Manager,
        init: A,
        mut f: impl FnMut(A, &RawName, EntryPlace) -> ControlFlow<B, A>,
    ) -> Result<ControlFlow<B, A>, SysError> {
        stack_trace!();
        let mut accum = init;
        let mut block_off = 0;
        loop {
            let (cid, cache) = match inode.get_nth_block(manager, block_off).await? {
                Ok(cache) => cache,
                Err(_list_len) => {
                    return Ok(try { accum });
                }
            };
            let r = cache
                .access_ro(|a| {
                    let r = a.iter().try_fold((accum, 0), |(b, off), raw| {
                        match f(b, raw, EntryPlace::new(block_off, cid, off)) {
                            ControlFlow::Continue(a) => try { (a, off + 1) },
                            ControlFlow::Break(b) => ControlFlow::Break(b),
                        }
                    });
                    match r {
                        ControlFlow::Continue((a, _)) => try { a },
                        ControlFlow::Break(b) => ControlFlow::Break(b),
                    }
                })
                .await;
            accum = match r {
                ControlFlow::Continue(a) => a,
                ControlFlow::Break(b) => return Ok(ControlFlow::Break(b)),
            };
            block_off += 1;
        }
    }
    async fn name_try_fold<A, B>(
        inode: &RawInode,
        manager: &Fat32Manager,
        init: A,
        mut f: impl FnMut(A, DirName) -> ControlFlow<B, A>,
    ) -> Result<ControlFlow<B, A>, SysError> {
        stack_trace!();
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
                    builder.push_long(long, place);
                    try { (accum, builder) }
                }
                Some(Name::Short(s)) => match f(accum, DirName::build_new(builder, s, place)) {
                    ControlFlow::Continue(accum) => {
                        builder.clear();
                        try { (accum, builder) }
                    }
                    ControlFlow::Break(b) => ControlFlow::Break(b),
                },
            },
        )
        .await?
        {
            ControlFlow::Continue((a, _)) => Ok(try { a }),
            ControlFlow::Break(b) => Ok(ControlFlow::Break(b)),
        }
    }
}

struct LongNameBuilder {
    long: Vec<[u16; 13]>,
    current: usize,
    start_place: Option<EntryPlace>,
    checksum: u8,
}

impl LongNameBuilder {
    const fn new() -> Self {
        Self {
            long: Vec::new(),
            current: 0,
            start_place: None,
            checksum: 0,
        }
    }
    fn success(&self) -> bool {
        self.current == 1
    }
    fn clear(&mut self) {
        self.long.clear();
    }
    fn push_long(&mut self, s: &RawLongName, start_place: EntryPlace) {
        if s.is_last() {
            self.current = s.order_num();
            self.start_place.replace(start_place);
            self.checksum = s.checksum();
        } else if self.current != s.order_num() + 1 || self.checksum != s.checksum() {
            self.current = 0;
        }
        if self.current == 0 {
            self.long.clear();
            self.start_place = None;
            self.checksum = 0;
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

fn entry_generate<'a>(
    long: &'a [[u16; 13]],
    short: &'a Align8<RawShortName>,
) -> impl Iterator<Item = RawName> + 'a {
    debug_assert!(long.len() <= 31);
    let checksum = short.checksum();
    return Iter {
        long,
        short,
        cnt: long.len() as isize,
        checksum,
    };

    struct Iter<'a> {
        long: &'a [[u16; 13]],
        short: &'a Align8<RawShortName>,
        cnt: isize,
        checksum: u8,
    }
    impl<'a> Iterator for Iter<'a> {
        type Item = RawName;
        fn next(&mut self) -> Option<Self::Item> {
            if self.cnt < 0 {
                return None;
            }
            if self.cnt == 0 {
                self.cnt -= 1;
                return Some(RawName::from_short(self.short));
            }
            let order = self.cnt as usize;
            let last = order == self.long.len();
            self.cnt -= 1;
            let src = &self.long[self.cnt as usize];
            let mut long = RawLongName::zeroed();
            long.set(src, order, last, self.checksum);
            Some(RawName::from_long(&long))
        }
    }
}

struct DirName {
    long: String,
    short: Align8<RawShortName>,
    start_place: EntryPlace,
    end_place: EntryPlace,
}

impl DirName {
    fn build_new(
        builder: &LongNameBuilder,
        short: &Align8<RawShortName>,
        end_place: EntryPlace,
    ) -> Self {
        if builder.success() {
            Self {
                long: builder.decode_utf16(),
                short: *short,
                start_place: builder.start_place.unwrap_or(end_place),
                end_place,
            }
        } else {
            Self {
                long: String::new(),
                short: *short,
                start_place: end_place,
                end_place,
            }
        }
    }
    fn attribute(&self) -> Attr {
        self.short.attributes
    }
    fn cid(&self) -> CID {
        let h16 = (self.short.cluster_h16 as u32) << 16;
        CID(h16 | self.short.cluster_l16 as u32)
    }
    fn take_name(self) -> String {
        if self.long.is_empty() {
            let buf = &mut [0; 12];
            let name = self.short.get_name(buf);
            String::from_utf8_lossy(name).into_owned()
        } else {
            self.long
        }
    }
    fn place(&self) -> (EntryPlace, EntryPlace) {
        (self.start_place, self.end_place)
    }
    // 存粹的短文件名没有小写字母
    fn short_same(&self, short: &([u8; 8], [u8; 3])) -> bool {
        self.long.is_empty() && &(self.short.name, self.short.ext) == short
    }
    // ".." or "."
    fn is_dot(&self) -> bool {
        self.long.is_empty()
            && [self.short.name[0], self.short.name[2], self.short.ext[0]] == [b'.', 0x20, 0x20]
            && [b'.', 0x20].contains(&self.short.name[1])
    }
    // 不区分大小写
    fn long_same(&self, str: &str) -> bool {
        if self.long.len() != str.len() {
            return false;
        }
        self.long
            .bytes()
            .zip(str.bytes())
            .all(|(a, b)| a.eq_ignore_ascii_case(&b))
    }
}
