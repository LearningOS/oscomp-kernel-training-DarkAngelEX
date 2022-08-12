use core::{ptr::NonNull, sync::atomic::AtomicUsize};

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use ftl_util::{
    async_tools::{ASysR, ASysRet},
    device::BlockDevice,
    error::{SysError, SysR, SysRet},
    fs::{stat::Stat, DentryType},
    list::InListNode,
    time::{Instant, TimeSpec},
};

use crate::{fssp::Fssp, select::PL};

pub trait FsInode: Send + Sync + 'static {
    // 类型转换

    fn block_device(&self) -> SysR<Arc<dyn BlockDevice>> {
        Err(SysError::ENOTBLK)
    }
    fn type_name(&self) -> &'static str {
        core::any::type_name::<Self>()
    }
    // === 共享操作 ===

    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn is_dir(&self) -> bool;
    fn ppoll(&self) -> PL {
        unimplemented!("poll {}", core::any::type_name::<Self>())
    }
    fn stat_fast(&self, _stat: &mut Stat) -> SysR<()> {
        SysR::Err(SysError::EAGAIN)
    }
    fn stat<'a>(&'a self, stat: &'a mut Stat) -> ASysR<()>;
    fn utimensat(&self, _times: [TimeSpec; 2], _now: fn() -> Instant) -> ASysR<()> {
        unimplemented!("utimensat {}", core::any::type_name::<Self>())
    }

    fn detach(&self) -> ASysR<()>;
    // === 目录操作 ===

    fn list(&self) -> ASysR<Vec<(DentryType, String)>>;
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>>;
    fn create<'a>(&'a self, name: &'a str, dir: bool, rw: (bool, bool)) -> ASysR<Box<dyn FsInode>>;
    fn place_inode<'a>(
        &'a self,
        _name: &'a str,
        _inode: Box<dyn FsInode>,
    ) -> ASysR<Box<dyn FsInode>> {
        Box::pin(async move {
            if !self.is_dir() {
                return Err(SysError::ENOTDIR);
            }
            panic!("place_inode unsupport: {}", core::any::type_name::<Self>())
        })
    }
    /// release: 释放资源, 当子节点为打开状态时为 false
    fn unlink_child<'a>(&'a self, name: &'a str, release: bool) -> ASysR<()>;
    fn rmdir_child<'a>(&'a self, name: &'a str) -> ASysR<()>;

    // === 文件操作 ===

    fn bytes(&self) -> SysRet;
    fn reset_data(&self) -> ASysR<()>;
    fn read_at_fast(
        &self,
        _buf: &mut [u8],
        _offset_with_ptr: (usize, Option<&AtomicUsize>),
    ) -> SysRet {
        Err(SysError::EAGAIN)
    }
    fn write_at_fast(
        &self,
        _buf: &[u8],
        _offset_with_ptr: (usize, Option<&AtomicUsize>),
    ) -> SysRet {
        Err(SysError::EAGAIN)
    }
    fn read_at<'a>(
        &'a self,
        buf: &'a mut [u8],
        offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet;
    fn write_at<'a>(
        &'a self,
        buf: &'a [u8],
        offset_with_ptr: (usize, Option<&'a AtomicUsize>),
    ) -> ASysRet;
}

inlist_access!(pub(crate) InodeFsspNode, VfsInode, fssp_node);

pub(crate) struct VfsInode {
    fssp: NonNull<Fssp>,
    fssp_node: InListNode<Self, InodeFsspNode>,
    pub fsinode: Box<dyn FsInode>,
}

unsafe impl Send for VfsInode {}
unsafe impl Sync for VfsInode {}

impl VfsInode {
    pub fn new(fssp: NonNull<Fssp>, inode: Box<dyn FsInode>) -> Arc<Self> {
        let mut ptr = Arc::new(Self {
            fssp,
            fssp_node: InListNode::new(),
            fsinode: inode,
        });
        unsafe {
            Arc::get_mut_unchecked(&mut ptr).fssp_node.init();
        }
        ptr
    }
    pub fn readable(&self) -> bool {
        self.fsinode.readable()
    }
    pub fn writable(&self) -> bool {
        self.fsinode.writable()
    }
    pub fn is_dir(&self) -> bool {
        self.fsinode.is_dir()
    }
    pub fn fsinode_ptr(&self) -> NonNull<dyn FsInode> {
        NonNull::new(self.fsinode.as_ref() as *const _ as *mut _).unwrap()
    }
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn fsinode_mut(&self) -> &mut dyn FsInode {
        &mut *self.fsinode_ptr().as_ptr()
    }
    pub fn vfsinode_ptr(&self) -> NonNull<Self> {
        NonNull::new(self as *const _ as *mut _).unwrap()
    }
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn vfsinode_mut(&self) -> &mut Self {
        &mut *self.vfsinode_ptr().as_ptr()
    }
    /// 只有文件可以运行
    pub async fn reset_data(&self) -> SysR<()> {
        self.fsinode.reset_data().await?;
        Ok(())
    }
    /// 此函数会在磁盘上判断是否重复
    ///
    /// 只有目录可以运行
    pub async fn create(&self, name: &str, dir: bool, rw: (bool, bool)) -> SysR<Arc<VfsInode>> {
        let fsinode = self.fsinode.create(name, dir, rw).await?;
        Ok(Self::new(self.fssp, fsinode))
    }
    pub async fn place_inode(&self, name: &str, inode: Box<dyn FsInode>) -> SysR<Arc<VfsInode>> {
        let fsinode = self.fsinode.place_inode(name, inode).await?;
        Ok(Self::new(self.fssp, fsinode))
    }
    /// 只有目录项可以运行
    pub async fn search(&self, name: &str) -> SysR<Arc<VfsInode>> {
        let fsinode = self.fsinode.search(name).await?;
        Ok(Self::new(self.fssp, fsinode))
    }
    /// 给inode增加析构时释放标志
    pub async fn detach(&self) -> SysR<()> {
        self.fsinode.detach().await
    }
    /// 这条路径的子节点不在缓存, 不能unlink目录!
    pub async fn unlink_child(&self, name: &str, release: bool) -> SysR<()> {
        debug_assert!(self.is_dir());
        self.fsinode.unlink_child(name, release).await
    }
    /// 这条路径的子节点不在缓存, 不能rmdir文件
    pub async fn rmdir_child(&self, name: &str) -> SysR<()> {
        debug_assert!(self.is_dir());
        self.fsinode.rmdir_child(name).await
    }
}
