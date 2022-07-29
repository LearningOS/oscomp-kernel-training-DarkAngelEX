use alloc::{collections::BTreeMap, sync::Arc};
use ftl_util::{
    error::{SysR, SysRet},
    fs::OpenFlags,
};
use vfs::File;

use crate::{
    config::USER_FNO_DEFAULT,
    syscall::{SysError, UniqueSysError},
    tools,
};

use super::resource::RLimit;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Fd(pub usize);
from_usize_impl!(Fd);

impl Fd {
    pub fn new(x: usize) -> Self {
        Self(x)
    }
    pub fn assert_eq(self, x: usize) {
        assert_eq!(self.0, x)
    }
    pub fn in_range(self) -> Result<(), UniqueSysError<{ SysError::EBADF as isize }>> {
        tools::bool_result(self.0 < i32::MAX as usize).map_err(|_| UniqueSysError)
    }
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FdSet {}

const F_LINUX_SPECIFIC_BASE: u32 = 1024;
const F_DUPFD: u32 = 0;
const F_DUPFD_CLOEXEC: u32 = F_LINUX_SPECIFIC_BASE + 6;
const F_GETFD: u32 = 1;
const F_SETFD: u32 = 2;
const F_GETFL: u32 = 3;
const F_SETFL: u32 = 4;
const F_GETLK: u32 = 5;
const F_SETLK: u32 = 6;
const F_SETLKW: u32 = 7;
const F_SETOWN: u32 = 8;
const F_GETOWN: u32 = 9;

#[derive(Clone)]
pub struct FdNode {
    file: Arc<dyn File>,
    close_on_exec: bool,
    op: OpenFlags,
}

#[derive(Clone)]
pub struct FdTable {
    map: BTreeMap<Fd, FdNode>,
    search_start: Fd,
    limit: RLimit,
}

impl FdTable {
    pub fn new() -> Self {
        let mut ret = Self {
            map: BTreeMap::new(),
            search_start: Fd(0),
            limit: USER_FNO_DEFAULT,
        };
        use crate::fs::stdio;
        // [0, 1, 2] => [stdin, stdout, stderr]
        ret.insert(Arc::new(stdio::Stdin), false, OpenFlags::empty())
            .unwrap()
            .assert_eq(0);
        ret.insert(Arc::new(stdio::Stdout), false, OpenFlags::empty())
            .unwrap()
            .assert_eq(1);
        ret.insert(Arc::new(stdio::Stdout), false, OpenFlags::empty())
            .unwrap()
            .assert_eq(2);
        ret
    }
    pub fn set_limit(&mut self, new: Option<RLimit>) -> SysR<RLimit> {
        let old = self.limit;
        if let Some(new) = new {
            new.check()?;
            self.limit = new;
        }
        Ok(old)
    }
    pub fn exec_run(&mut self) {
        let mut search = Fd(0);
        let mut find = false;
        self.map.retain(|&fd, n| {
            if !find {
                if fd != search || n.close_on_exec {
                    find = true;
                } else {
                    search = fd.next();
                }
            }
            !n.close_on_exec
        });
    }
    fn alloc_fd(&mut self) -> SysR<Fd> {
        self.alloc_fd_min(Fd(0))
    }
    /// 寻找不小于min的最小Fd
    fn alloc_fd_min(&mut self, min: Fd) -> SysR<Fd> {
        if self.map.len() >= self.limit.rlim_max {
            return Err(SysError::EMFILE);
        }
        let Fd(mut min) = min.max(self.search_start);
        let search_from_start = Fd(min) == self.search_start;
        for fd in self.map.range(Fd(min)..).map(|(&Fd(fd), _b)| fd) {
            if fd == min {
                min += 1;
            } else {
                break;
            }
        }
        if search_from_start {
            self.search_start = Fd(min);
        }
        Ok(Fd(min))
    }
    /// 自动选择
    pub fn insert(&mut self, file: Arc<dyn File>, close_on_exec: bool, op: OpenFlags) -> SysR<Fd> {
        self.insert_min(Fd(0), file, close_on_exec, op)
    }
    /// 寻找不小于min的最小fd并插入
    pub fn insert_min(
        &mut self,
        min: Fd,
        file: Arc<dyn File>,
        close_on_exec: bool,
        op: OpenFlags,
    ) -> SysR<Fd> {
        let fd = self.alloc_fd_min(min)?;
        let node = FdNode {
            file,
            close_on_exec,
            op,
        };
        self.map
            .try_insert(fd, node)
            .ok()
            .expect("FdTable double insert same fd");
        Ok(fd)
    }
    /// 覆盖存在的文件
    pub fn set_insert(&mut self, fd: Fd, file: Arc<dyn File>, close_on_exec: bool, op: OpenFlags) {
        let node = FdNode {
            file,
            close_on_exec,
            op,
        };
        let _ = self.map.insert(fd, node);
        self.search_start = self.search_start.min(fd.next());
    }
    pub fn get(&self, fd: Fd) -> Option<&Arc<dyn File>> {
        self.map.get(&fd).map(|n| &n.file)
    }
    pub fn get_node(&self, fd: Fd) -> Option<&FdNode> {
        self.map.get(&fd)
    }
    pub fn fcntl(&mut self, fd: Fd, cmd: u32, arg: usize) -> SysRet {
        const FD_CLOEXEC: usize = 1;
        let node = self.map.get_mut(&fd).ok_or(SysError::EBADF)?;
        match cmd {
            // 复制文件描述符
            F_DUPFD | F_DUPFD_CLOEXEC => {
                let min = Fd(arg);
                let file = node.file.clone();
                let close_on_exec = node.close_on_exec;
                let op = node.op;
                let fd = self.insert_min(min, file, close_on_exec, op)?;
                Ok(fd.0)
            }
            F_GETFD => Ok(if node.close_on_exec { FD_CLOEXEC } else { 0 }),
            F_SETFD => {
                node.close_on_exec = arg & FD_CLOEXEC != 0;
                Ok(0)
            }
            F_GETFL => Ok(node.op.bits() as usize),
            F_SETFL => {
                node.op = OpenFlags::from_bits_truncate(arg as u32);
                node.close_on_exec = arg & FD_CLOEXEC != 0;
                Ok(0)
            }
            F_GETLK => todo!(),
            F_SETLK => todo!(),
            F_SETLKW => todo!(),
            F_SETOWN => todo!(),
            F_GETOWN => todo!(),
            unknown => todo!("fcntl unknown cmd: {}", unknown),
        }
    }
    pub fn remove(&mut self, fd: Fd) -> Option<Arc<dyn File>> {
        self.search_start = self.search_start.min(fd);
        let file = self.map.remove(&fd);
        file.map(|n| n.file)
    }
    pub fn dup(&mut self, fd: Fd) -> SysR<Fd> {
        let file = self.get_node(fd).ok_or(SysError::EBADF)?.clone();
        let new_fd = self.insert(file.file, false, file.op)?;
        Ok(new_fd)
    }
    pub fn replace_dup(&mut self, old_fd: Fd, new_fd: Fd, flags: OpenFlags) -> SysR<()> {
        if old_fd == new_fd {
            return Err(SysError::EINVAL);
        }
        let file = self.get(old_fd).ok_or(SysError::EBADF)?.clone();
        let close_on_exec = flags.contains(OpenFlags::CLOEXEC);
        // close previous file
        let _ = self.map.insert(
            new_fd,
            FdNode {
                file,
                close_on_exec,
                op: flags,
            },
        );
        Ok(())
    }
}
