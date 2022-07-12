use crate::{
    memory::user_ptr::{UserReadPtr, UserWritePtr},
    syscall::{fs::PRINT_SYSCALL_FS, SysError, SysRet, Syscall},
    user::check::UserCheck,
};

impl Syscall<'_> {
    ///
    ///
    ///
    pub async fn sys_mount(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_mount");
        }
        let (src, dst, mount_type, _flags, _data): (
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            u32,
            UserReadPtr<u8>,
        ) = self.cx.into();
        let _src = UserCheck::new(self.process).array_zero_end(src).await?;
        let _dst = UserCheck::new(self.process).array_zero_end(dst).await?;
        let _mount_type = UserCheck::new(self.process)
            .array_zero_end(mount_type)
            .await?;
        if false {
            println!("sys_mount unimplement");
            return Err(SysError::ENOSYS);
        }
        Ok(0)
    }
    pub async fn sys_statfs(&mut self) -> SysRet {
        stack_trace!();
        if PRINT_SYSCALL_FS {
            println!("sys_statfs");
        }

        #[derive(Clone, Copy)]
        struct StatFs {
            f_type: usize,       /* Type of filesystem (see below) */
            f_bsize: usize,      /* Optimal transfer block size */
            f_blocks: usize,     /* Total data blocks in filesystem */
            f_bfree: usize,      /* Free blocks in filesystem */
            f_bavail: usize,     /* Free blocks available to unprivileged user */
            f_files: usize,      /* Total inodes in filesystem */
            f_ffree: usize,      /* Free inodes in filesystem */
            f_fsid: usize,       /* Filesystem ID */
            f_namelen: usize,    /* Maximum length of filenames */
            f_frsize: usize,     /* Fragment size (since Linux 2.6) */
            f_flags: usize,      /* Mount flags of filesystem (since Linux 2.6.36) */
            f_spare: [usize; 4], /* Padding bytes reserved for future use */
        }
        let (path, buf): (UserReadPtr<u8>, UserWritePtr<StatFs>) = self.cx.into();
        let path = UserCheck::new(self.process)
            .array_zero_end(path)
            .await?
            .access()
            .to_vec();
        match path.as_slice() {
            b"/" => (),
            _ => unimplemented!(),
        }
        let buf = UserCheck::new(self.process).writable_value(buf).await?;
        buf.store(StatFs {
            f_type: 0,
            f_bsize: 4096,
            f_blocks: 100,
            f_bfree: 100,
            f_bavail: 100,
            f_files: 100,
            f_ffree: 100,
            f_fsid: 100,
            f_namelen: 100,
            f_frsize: 100,
            f_flags: 100,
            f_spare: [100; _],
        });
        Ok(0)
    }
    pub async fn sys_umount2(&mut self) -> SysRet {
        let (target, _flags): (UserReadPtr<u8>, u32) = self.cx.into();
        let _target = UserCheck::new(self.process).array_zero_end(target).await?;
        Ok(0)
    }
}
