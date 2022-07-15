use core::{f64::RADIX, intrinsics::size_of, iter::Cloned};

use alloc::{
    boxed::Box,
    collections::{BTreeMap, VecDeque},
    string::String,
    sync::Arc,
    vec::{self, Vec},
};
use ftl_util::{
    async_tools::ASysRet,
    error::{SysError, SysRet},
    fs::{File, OpenFlags},
    sync::Spin,
};

use crate::{
    config::USER_STACK_SIZE,
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    process::fd::Fd,
    sync::mutex::{SpinLock, SpinNoIrqLock},
    user::check::UserCheck,
    xdebug::{PRINT_SYSCALL, PRINT_SYSCALL_ALL},
};

use super::Syscall;

const PRINT_SYSCALL_NET: bool = true && PRINT_SYSCALL || PRINT_SYSCALL_ALL;
const AF_INET: u32 = 2;
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SaFamily(u32);
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct SocketAddr {
    sa_family: SaFamily,
    sa_data: [u8; 14],
}

struct SocketFile {
    file: SpinNoIrqLock<VecDeque<u8>>,
}

impl SocketFile {
    fn new() -> Arc<Self> {
        Arc::new(SocketFile {
            file: SpinNoIrqLock::new(VecDeque::new()),
        })
    }
}

struct SocketDataBuffer {
    socket_buf: BTreeMap<SocketAddr, Arc<dyn File>>,
}

impl SocketDataBuffer {
    const fn new() -> Self {
        SocketDataBuffer {
            socket_buf: BTreeMap::new(),
        }
    }
}

static SOCKET_BUF: SpinNoIrqLock<SocketDataBuffer> = SpinNoIrqLock::new(SocketDataBuffer::new());

impl File for SocketFile {
    fn readable(&self) -> bool {
        todo!()
    }

    fn writable(&self) -> bool {
        todo!()
    }

    fn read<'a>(&'a self, write_only: &'a mut [u8]) -> ASysRet {
        Box::pin(async move {
            let mut lk = self.file.lock();
            // println!("lk.len : {}", lk.len());
            let len = lk.len().min(write_only.len());
            // println!("read deque len : {}", len);
            lk.drain(..len)
                .zip(write_only.iter_mut())
                .for_each(|(a, b)| *b = a);
            // println!("write_only:\n{:?}", write_only);
            Ok(len)
        })
    }

    fn write<'a>(&'a self, read_only: &'a [u8]) -> ASysRet {
        // println!("read_only len : {}",read_only.len());
        Box::pin(async move {
            let mut lk = self.file.lock();
            read_only.iter().for_each(|&v| lk.push_back(v));
            // println!("write len:{}", read_only.len());
            // println!("after write : {:?}", read_only);
            Ok(read_only.len())
        })
    }

    fn to_vfs_inode(&self) -> ftl_util::error::SysR<&dyn ftl_util::fs::VfsInode> {
        Err(SysError::ENOTDIR)
    }

    fn can_mmap(&self) -> bool {
        self.can_read_offset() && self.can_write_offset()
    }

    fn can_read_offset(&self) -> bool {
        false
    }

    fn can_write_offset(&self) -> bool {
        false
    }

    fn lseek(&self, _offset: isize, _whence: ftl_util::fs::Seek) -> SysRet {
        unimplemented!("lseek {}", core::any::type_name::<Self>())
    }

    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysRet {
        unimplemented!("read_at {}", core::any::type_name::<Self>())
    }

    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysRet {
        unimplemented!("write_at {}", core::any::type_name::<Self>())
    }

    fn ioctl(&self, _cmd: u32, _arg: usize) -> SysRet {
        Ok(0)
    }

    fn stat<'a>(&'a self, _stat: &'a mut ftl_util::fs::stat::Stat) -> fat32::ASysR<()> {
        Box::pin(async move { Err(SysError::EACCES) })
    }

    fn utimensat(&self, _times: [ftl_util::time::TimeSpec; 2]) -> ASysRet {
        unimplemented!("utimensat {}", core::any::type_name::<Self>())
    }
}

impl Syscall<'_> {
    pub fn sys_socket(&mut self) -> SysRet {
        stack_trace!();
        let (domain, ty, protocol): (u32, u32, u32) = self.cx.into();
        if PRINT_SYSCALL_NET {
            println!(
                "sys_socket\n\t domain : {:#x}, type : {:#x}, ctid : {:#x}",
                domain, ty, protocol
            );
        }
        let file = SocketFile::new();
        self.alive_lock()
            .fd_table
            .insert(file, true, OpenFlags::CLOEXEC | OpenFlags::NONBLOCK)
            .map(|fd| fd.0)
    }

    pub fn sys_bind(&mut self) -> SysRet {
        stack_trace!();
        Ok(0)
    }

    pub fn sys_getsockname(&mut self) -> SysRet {
        stack_trace!();
        Ok(0)
    }

    pub fn sys_setsockopt(&mut self) -> SysRet {
        stack_trace!();
        Ok(0)
    }

    pub async fn sys_sendto(&mut self) -> SysRet {
        stack_trace!();
        let (fd, buf, len, _flag, sa, _sa_size): (
            u32,
            UserReadPtr<u8>,
            usize,
            u32,
            UserReadPtr<SocketAddr>,
            usize,
        ) = self.cx.into();
        let file = self
            .alive_then(|a| a.fd_table.get(Fd(fd as usize)).cloned())
            .ok_or(SysError::EBADF)?;
        let buf = UserCheck::new(self.process)
            .readonly_slice(buf, len)
            .await?;
        // println!("before write : {}", buf.len());
        let len = file.write(&*buf.access()).await?;
        let addr = UserCheck::new(self.process)
            .readonly_value(sa)
            .await?
            .load();
        SOCKET_BUF.lock().socket_buf.insert(addr, file);
        Ok(len)
    }

    pub async fn sys_recvfrom(&mut self) -> SysRet {
        stack_trace!();
        let (_fd, buf, len, _flag, sa, _addr_len): (
            u32,
            UserWritePtr<u8>,
            usize,
            u32,
            UserReadPtr<SocketAddr>,
            UserReadPtr<usize>,
        ) = self.cx.into();
        let buf = UserCheck::new(self.process)
            .writable_slice(buf, len)
            .await?;
        let addr = UserCheck::new(self.process)
            .readonly_value(sa)
            .await?
            .load();
        let file = SOCKET_BUF.lock().socket_buf.get(&addr).unwrap().clone();

        let len = file.read(&mut *buf.access_mut()).await?;

        Ok(len)
    }

    pub fn sys_listen(&mut self) -> SysRet {
        stack_trace!();
        Ok(0)
    }

    pub fn sys_connect(&mut self) -> SysRet {
        stack_trace!();
        Ok(0)
    }

    pub fn sys_accept(&mut self) -> SysRet {
        stack_trace!();
        Ok(0)
    }
}
