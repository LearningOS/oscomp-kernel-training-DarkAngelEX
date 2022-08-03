use core::{convert::TryFrom, sync::atomic::AtomicU32};

use alloc::vec::Vec;
use ftl_util::error::SysR;

use crate::{
    config::PAGE_SIZE,
    local,
    memory::{
        address::{PageCount, UserAddr},
        allocator::frame,
        user_ptr::{Read, UserPtr, UserReadPtr, UserWritePtr, Write},
    },
    process::Process,
    syscall::SysError,
};

use super::{check_impl::UserCheckImpl, AutoSum, UserData, UserDataMut, UserType};

pub struct UserCheck<'a> {
    process: &'a Process,
    _auto_sum: AutoSum,
}

unsafe impl Send for UserCheck<'_> {}
unsafe impl Sync for UserCheck<'_> {}

impl<'a> UserCheck<'a> {
    pub fn new(process: &'a Process) -> Self {
        assert!(local::always_local().user_access_status.is_forbid());
        Self {
            process,
            _auto_sum: AutoSum::new(),
        }
    }
    pub async fn array_zero_end<T>(&self, ptr: UserReadPtr<T>) -> SysR<UserData<T>>
    where
        T: UserType,
    {
        let allocator = &mut frame::default_allocator();
        // misalign check
        if ptr.as_usize() % core::mem::size_of::<T>() != 0 {
            return Err(SysError::EFAULT);
        }
        let mut uptr = UserAddr::try_from(ptr)?;

        let check_impl = UserCheckImpl::new(self.process);
        check_impl.read_check_async(ptr, allocator).await?;

        let mut len = 0;
        let mut ch_is_null = move || {
            let ch: T = unsafe { *uptr.as_ptr() }; // if access fault, return 0.
            uptr.add_assign(core::mem::size_of::<T>());
            ch.is_null()
        };
        // check first access
        if ch_is_null() {
            let slice = unsafe { &*core::ptr::slice_from_raw_parts(ptr.raw_ptr(), 0) };
            return Ok(UserData::new(slice));
        } else {
            len += 1;
        }
        loop {
            let nxt_ptr = ptr.offset(len as isize);
            if nxt_ptr.as_usize() % PAGE_SIZE == 0 {
                check_impl.read_check_async(nxt_ptr, allocator).await?;
            }
            if ch_is_null() {
                break;
            }
            len += 1;
            // check when first access a page.
        }
        let slice = unsafe { &*core::ptr::slice_from_raw_parts(ptr.raw_ptr(), len) };
        Ok(UserData::new(slice))
    }
    /// return a slice witch len == 1
    pub async fn readonly_value<T: Copy, P: Read>(&self, ptr: UserPtr<T, P>) -> SysR<UserData<T>> {
        self.readonly_slice(ptr, 1).await
    }
    pub async fn readonly_value_nullable<T: Copy, P: Read>(
        &self,
        ptr: UserPtr<T, P>,
    ) -> SysR<Option<UserData<T>>> {
        if ptr.is_null() {
            Ok(None)
        } else {
            Some(self.readonly_slice(ptr, 1).await).transpose()
        }
    }
    /// return a slice witch len == 1
    pub async fn writable_value<T: Copy, P: Write>(
        &self,
        ptr: UserPtr<T, P>,
    ) -> SysR<UserDataMut<T>> {
        self.writable_slice(ptr, 1).await
    }
    pub async fn array_2d_zero_end<T: UserType>(
        &self,
        ptr: UserReadPtr<UserReadPtr<T>>,
    ) -> SysR<Vec<UserData<T>>> {
        if ptr.is_null() {
            return Ok(Vec::new());
        }
        let arr_1d = self.array_zero_end(ptr).await?;
        let mut ret = Vec::new();
        for &arr_2d in &*arr_1d.access() {
            ret.push(self.array_zero_end(arr_2d).await?);
        }
        Ok(ret)
    }

    pub async fn readonly_slice<T: Copy, P: Read>(
        &self,
        ptr: UserPtr<T, P>,
        len: usize,
    ) -> SysR<UserData<T>> {
        if ptr.as_usize() % core::mem::align_of::<T>() != 0 {
            return Err(SysError::EFAULT);
        }
        let allocator = &mut frame::default_allocator();
        let mut cur = UserAddr::try_from(ptr)?.floor();
        let uend4k = UserAddr::try_from(ptr.offset(len as isize))?.ceil();
        let check_impl = UserCheckImpl::new(self.process);
        while cur != uend4k {
            let cur_ptr = UserReadPtr::from_usize(cur.into_usize());
            // if error occur will change status by exception
            check_impl
                .read_check_async::<u8>(cur_ptr, allocator)
                .await?;
            cur.add_page_assign(PageCount(1));
        }
        let slice = core::ptr::slice_from_raw_parts(ptr.raw_ptr(), len);
        Ok(UserData::new(unsafe { &*slice }))
    }
    pub async fn readonly_slice_nullable<T: Copy, P: Read>(
        &self,
        ptr: UserPtr<T, P>,
        len: usize,
    ) -> SysR<Option<UserData<T>>> {
        if ptr.is_null() {
            Ok(None)
        } else {
            Some(self.readonly_slice(ptr, len).await).transpose()
        }
    }
    pub async fn writable_slice<T: Copy, P: Write>(
        &self,
        ptr: UserPtr<T, P>,
        len: usize,
    ) -> SysR<UserDataMut<T>> {
        // println!("tran 0");
        let allocator = &mut frame::default_allocator();
        if ptr.as_usize() % core::mem::align_of::<T>() != 0 {
            println!(
                "[kernel]user write ptr check fail: no align. ptr: {:#x} align: {}",
                ptr.as_usize(),
                core::mem::align_of::<T>()
            );
            return Err(SysError::EFAULT);
        }
        let mut cur = UserAddr::try_from(ptr)?.floor();
        let uend4k = UserAddr::try_from(ptr.offset(len as isize))?.ceil();
        let check_impl = UserCheckImpl::new(self.process);
        while cur != uend4k {
            let cur_ptr = UserWritePtr::from_usize(cur.into_usize());
            check_impl
                .write_check_async::<u8>(cur_ptr, allocator)
                .await?;
            cur.add_page_assign(PageCount(1));
        }
        let slice = core::ptr::slice_from_raw_parts_mut(ptr.raw_ptr_mut(), len);
        Ok(UserDataMut::new(slice))
    }
    pub async fn writable_slice_nullable<T: Copy, P: Write>(
        &self,
        ptr: UserPtr<T, P>,
        len: usize,
    ) -> SysR<Option<UserDataMut<T>>> {
        if ptr.is_null() {
            Ok(None)
        } else {
            Some(self.writable_slice(ptr, len).await).transpose()
        }
    }
    pub async fn atomic_u32<P: Write>(&self, ptr: UserPtr<u32, P>) -> SysR<UserDataMut<AtomicU32>> {
        if ptr.as_usize() % core::mem::align_of::<u32>() != 0 {
            println!(
                "[kernel]user atomic_u32 check fail: no align. ptr: {:#x} align: {}",
                ptr.as_usize(),
                core::mem::align_of::<u32>()
            );
            return Err(SysError::EFAULT);
        }
        let check_impl = UserCheckImpl::new(self.process);
        check_impl
            .atomic_u32_check_async(
                UserWritePtr::from_usize(ptr.as_usize()),
                &mut frame::default_allocator(),
            )
            .await?;
        let slice = core::ptr::slice_from_raw_parts_mut(ptr.raw_ptr_mut().cast(), 1);
        Ok(UserDataMut::new(slice))
    }
}
