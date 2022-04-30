pub mod inode;
pub mod manager;
pub mod super_block;

pub fn init() {
    inode::init();
}
