//文件类型
pub const S_IFMT: u32 = 0o170000; // 文件类型掩码
pub const S_IFSOCK: u32 = 0o140000; // socket
pub const S_IFLNK: u32 = 0o120000; // 链接文件
pub const S_IFREG: u32 = 0o100000; // 普通文件
pub const S_IFBLK: u32 = 0o060000; // 块设备
pub const S_IFNAM: u32 = 0o050000; // 名字文件
pub const S_IFDIR: u32 = 0o040000; // 目录文件
pub const S_IFCHR: u32 = 0o020000; // 字符设备文件
pub const S_IFIFO: u32 = 0o010000; // FIFO

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Stat {
    pub st_dev: u64,   /* ID of device containing file -文件所在设备的ID*/
    pub st_ino: u64,   /* inode number -inode节点号*/
    pub st_mode: u32,  /* protection -保护模式?*/
    pub st_nlink: u32, /* number of hard links -链向此文件的连接数(硬连接)*/
    pub st_uid: u32,   /* user ID of owner -user id*/
    pub st_gid: u32,   /* group ID of owner - group id*/
    pub st_rdev: u64,  /* device ID (if special file) -设备号，针对设备文件*/
    pub __pad1: usize,
    pub st_size: usize,  /* total size, in bytes -文件大小，字节为单位*/
    pub st_blksize: u32, /* blocksize for filesystem I/O -系统块的大小*/
    pub __pad2: u32,
    pub st_blocks: u64,  /* number of 512B blocks allocated -文件所占块数*/
    pub st_atime: usize, /* time of last access -最近存取时间*/
    pub st_atime_nsec: usize,
    pub st_mtime: usize, /* time of last modification -最近修改时间*/
    pub st_mtime_nsec: usize,
    pub st_ctime: usize, /* time of last status change - */
    pub st_ctime_nsec: usize,
    pub __unused4: u32,
    pub __unused5: u32,
}

impl Stat {
    pub fn zeroed() -> Self {
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
}
