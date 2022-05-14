#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DentryType {
    UNKNOWN = 0,
    FIFO = 1,  // pipe
    CHR = 2,   // character device
    DIR = 4,   // directory
    BLK = 6,   // block device
    REG = 8,   // regular file
    LNK = 10,  // symbolic link
    SOCK = 12, // UNIX domain socket
}