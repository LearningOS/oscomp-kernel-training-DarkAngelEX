use core::ptr::NonNull;

use alloc::{boxed::Box, sync::Arc};
use ftl_util::{async_tools::ASysR, error::SysR, list::InListNode};

use crate::fssp::Fssp;

pub trait FsInode: Send + Sync + 'static {
    fn is_dir(&self) -> bool;
    fn search<'a>(&'a self, name: &'a str) -> ASysR<Box<dyn FsInode>>;
    fn create<'a>(&'a self, name: &'a str, dir: bool) -> ASysR<Box<dyn FsInode>>;
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
    pub fn is_dir(&self) -> bool {
        self.fsinode.is_dir()
    }
    pub fn fsinode_ptr(&self) -> NonNull<dyn FsInode> {
        NonNull::new(self.fsinode.as_ref() as *const _ as *mut _).unwrap()
    }
    pub unsafe fn fsinode_mut(&self) -> &mut dyn FsInode {
        &mut *self.fsinode_ptr().as_ptr()
    }
    pub fn vfsinode_ptr(&self) -> NonNull<Self> {
        NonNull::new(self as *const _ as *mut _).unwrap()
    }
    pub unsafe fn vfsinode_mut(&self) -> &mut Self {
        &mut *self.vfsinode_ptr().as_ptr()
    }
    /// 只有文件可以运行
    pub async fn reset_data(&self) -> SysR<()> {
        todo!()
    }
    /// 此函数会在磁盘上判断是否重复
    ///
    /// 只有目录可以运行
    pub async fn create(&self, name: &str, dir: bool) -> SysR<Arc<VfsInode>> {
        let fsinode = self.fsinode.create(name, dir).await?;
        Ok(Self::new(self.fssp, fsinode))
    }
    ///
    ///
    pub async fn search(&self, name: &str) -> SysR<Arc<VfsInode>> {
        let fsinode = self.fsinode.search(name).await?;
        Ok(Self::new(self.fssp, fsinode))
    }
}
