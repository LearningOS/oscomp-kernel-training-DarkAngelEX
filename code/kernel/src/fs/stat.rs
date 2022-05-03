#[repr(C)]
#[derive(Clone, Copy)]
pub struct Stat {
    pub st_dev: u64,   /* ID of device containing file -文件所在设备的ID*/
    pub st_ino: u64,   /* inode number -inode节点号*/
    pub st_mode: u32,  /* protection -保护模式?*/
    pub st_nlink: u32, /* number of hard links -链向此文件的连接数(硬连接)*/
    pub st_uid: u32,   /* user ID of owner -user id*/
    pub st_gid: u32,   /* group ID of owner - group id*/
    pub st_rdev: u64,  /* device ID (if special file) -设备号，针对设备文件*/
    __pad1: u32,
    pub st_size: u32,    /* total size, in bytes -文件大小，字节为单位*/
    pub st_blksize: u32, /* blocksize for filesystem I/O -系统块的大小*/
    __pad2: u32,
    pub st_blocks: u64, /* number of 512B blocks allocated -文件所占块数*/
    pub st_atime: u32,  /* time of last access -最近存取时间*/
    pub st_atime_nsec: u32,
    pub st_mtime: u32, /* time of last modification -最近修改时间*/
    pub st_mtime_nsec: u32,
    pub st_ctime: u32, /* time of last status change - */
    pub st_ctime_nsec: u32,
    __unused4: u32,
    __unused5: u32,
}

impl Stat {
    pub fn zeroed() -> Self {
        unsafe { core::mem::MaybeUninit::zeroed().assume_init() }
    }
}
