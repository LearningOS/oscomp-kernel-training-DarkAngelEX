use alloc::{collections::BTreeMap, sync::Arc};

use crate::{
    fs::{File, Stdin, Stdout},
    syscall::{SysError, UniqueSysError},
    tools,
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Fd(usize);
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

#[derive(Clone)]
struct FdNode {
    file: Arc<dyn File>,
    close_on_exec: bool,
}

#[derive(Clone)]
pub struct FdTable {
    map: BTreeMap<Fd, FdNode>,
    search_start: Fd,
}

impl FdTable {
    pub fn new() -> Self {
        let mut ret = Self {
            map: BTreeMap::new(),
            search_start: Fd(0),
        };
        ret.insert(Arc::new(Stdin), false).assert_eq(0);
        ret.insert(Arc::new(Stdout), false).assert_eq(1);
        ret.insert(Arc::new(Stdout), false).assert_eq(2);
        ret
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
    fn alloc_fd(&mut self) -> Fd {
        let mut cur = self.search_start;
        for &fd in self.map.keys() {
            if cur == fd {
                cur.0 += 1;
            } else {
                self.search_start = cur.next();
                return cur;
            }
        }
        cur.in_range().unwrap();
        self.search_start = cur.next();
        cur
    }
    /// 自动选择
    pub fn insert(&mut self, file: Arc<dyn File>, close_on_exec: bool) -> Fd {
        let fd = self.alloc_fd();
        let node = FdNode {
            file,
            close_on_exec,
        };
        self.map
            .try_insert(fd, node)
            .ok()
            .expect("FdTable double insert same fd");
        fd
    }
    /// 覆盖存在的文件
    pub fn set_insert(&mut self, fd: Fd, file: Arc<dyn File>, close_on_exec: bool) {
        let node = FdNode {
            file,
            close_on_exec,
        };
        let _ = self.map.insert(fd, node);
        self.search_start = self.search_start.min(fd.next());
    }
    pub fn get(&self, fd: Fd) -> Option<&Arc<dyn File>> {
        self.map.get(&fd).map(|n| &n.file)
    }
    pub fn remove(&mut self, fd: Fd) -> Option<Arc<dyn File>> {
        self.search_start = self.search_start.min(fd);
        let file = self.map.remove(&fd);
        file.map(|n| n.file)
    }
    pub fn dup(&mut self, fd: Fd) -> Option<Fd> {
        let file = self.get(fd)?.clone();
        let new_fd = self.insert(file.clone(), false);
        Some(new_fd)
    }
    pub fn replace_dup(
        &mut self,
        old_fd: Fd,
        new_fd: Fd,
        close_on_exec: bool,
    ) -> Result<(), SysError> {
        if old_fd == new_fd {
            return Err(SysError::EINVAL);
        }
        let file = self.get(old_fd).ok_or(SysError::EBADF)?.clone();
        let node = FdNode {
            file,
            close_on_exec,
        };
        // close previous file
        let _ = self.map.insert(new_fd, node);
        Ok(())
    }
}
