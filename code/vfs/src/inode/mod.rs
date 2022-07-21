use alloc::{boxed::Box, sync::Arc};
use ftl_util::list::InListNode;

pub trait FsInode: Send + Sync + 'static {}

inlist_access!(pub(crate) InodeFsspNode, VfsInode, fssp_node);

pub struct VfsInode {
    fssp_node: InListNode<Self, InodeFsspNode>,
    inode: Box<dyn FsInode>,
}

impl VfsInode {
    pub fn new(inode: Box<dyn FsInode>) -> Arc<Self> {
        let mut ptr = Arc::new(Self {
            fssp_node: InListNode::new(),
            inode,
        });
        unsafe {
            Arc::get_mut_unchecked(&mut ptr).fssp_node.init();
        }
        ptr
    }
}
