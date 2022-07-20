use alloc::boxed::Box;
use ftl_util::list::InListNode;

pub trait FsInode: Send + Sync + 'static {}

inlist_access!(InodeFsspNode, VfsInode, fssp_node);

pub struct VfsInode {
    fssp_node: InListNode<Self, InodeFsspNode>,
    inode: Box<dyn FsInode>,
}

impl VfsInode {}
