mod inode;
mod stdio;

pub trait File: Send + Sync {
    fn readable(&self) -> bool;
    fn writable(&self) -> bool;
    fn read(&self, buf: UserData<u8>) -> usize;
    fn write(&self, buf: UserData<u8>) -> usize;
}

pub use inode::{list_apps, open_file, OSInode, OpenFlags};
pub use stdio::{Stdin, Stdout};

use crate::user::UserData;
