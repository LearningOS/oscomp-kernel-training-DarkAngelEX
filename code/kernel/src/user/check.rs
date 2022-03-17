use core::convert::TryFrom;

use alloc::vec::Vec;

use crate::{
    config::PAGE_SIZE,
    local,
    memory::{
        address::{PageCount, UserAddr},
        user_ptr::{UserReadPtr, UserWritePtr},
    },
    syscall::{SysError, UniqueSysError},
};

use super::{check_impl::UserCheckImpl, AutoSum, UserData, UserDataMut, UserType};

pub struct UserCheck(AutoSum);

unsafe impl Send for UserCheck {}
unsafe impl Sync for UserCheck {}

impl UserCheck {
    pub fn new() -> Self {
        assert!(local::always_local().user_access_status.is_forbid());
        Self(AutoSum::new())
    }
    pub async fn translated_user_array_zero_end<T>(
        &self,
        ptr: UserReadPtr<T>,
    ) -> Result<UserData<T>, UniqueSysError<{ SysError::EFAULT as isize }>>
    where
        T: UserType,
    {
        // misalign check
        if ptr.as_usize() % core::mem::size_of::<T>() != 0 {
            return Err(UniqueSysError);
        }
        let mut uptr = UserAddr::try_from(ptr)?;

        let check_impl = UserCheckImpl::new();
        check_impl.read_check(ptr).await?;

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
                check_impl.read_check(nxt_ptr).await?;
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

    pub async fn translated_user_2d_array_zero_end<T: UserType>(
        &self,
        ptr: UserReadPtr<UserReadPtr<T>>,
    ) -> Result<Vec<UserData<T>>, UniqueSysError<{ SysError::EFAULT as isize }>> {
        let arr_1d = self.translated_user_array_zero_end(ptr).await?;
        let mut ret = Vec::new();
        for &arr_2d in &*arr_1d.access() {
            ret.push(self.translated_user_array_zero_end(arr_2d).await?);
        }
        Ok(ret)
    }

    pub async fn translated_user_readonly_slice<T: UserType>(
        &self,
        ptr: UserReadPtr<T>,
        len: usize,
    ) -> Result<UserData<T>, UniqueSysError<{ SysError::EFAULT as isize }>> {
        if ptr.as_usize() % core::mem::size_of::<T>() != 0 {
            return Err(UniqueSysError);
        }
        let ubegin = UserAddr::try_from(ptr)?;
        let uend = UserAddr::try_from(ptr.offset(len as isize))?;
        let mut cur = ubegin.floor();
        let uend4k = uend.ceil();
        let check_impl = UserCheckImpl::new();
        while cur != uend4k {
            let cur_ptr = UserReadPtr::from_usize(cur.into_usize());
            // if error occur will change status by exception
            check_impl.read_check::<u8>(cur_ptr).await?;
            cur.add_page_assign(PageCount::from_usize(1));
        }
        let slice = core::ptr::slice_from_raw_parts(ptr.raw_ptr(), len);
        Ok(UserData::new(unsafe { &*slice }))
    }

    pub async fn translated_user_writable_slice<T: UserType>(
        &self,
        ptr: UserWritePtr<T>,
        len: usize,
    ) -> Result<UserDataMut<T>, UniqueSysError<{ SysError::EFAULT as isize }>> {
        if ptr.as_usize() % core::mem::size_of::<T>() != 0 {
            return Err(UniqueSysError);
        }
        let ubegin = UserAddr::try_from(ptr)?;
        let uend = UserAddr::try_from(ptr.offset(len as isize))?;
        let mut cur = ubegin.floor();
        let uend4k = uend.ceil();
        let check_impl = UserCheckImpl::new();
        while cur != uend4k {
            let cur_ptr = UserWritePtr::from_usize(cur.into_usize());
            check_impl.write_check::<u8>(cur_ptr).await?;
            cur.add_page_assign(PageCount::from_usize(1));
        }
        let slice = core::ptr::slice_from_raw_parts_mut(ptr.raw_ptr_mut(), len);
        Ok(UserDataMut::new(slice))
    }
}
