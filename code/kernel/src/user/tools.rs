use core::convert::TryFrom;

use alloc::vec::Vec;

use crate::{
    local,
    memory::address::{PageCount, UserAddr},
    syscall::{SysError, UniqueSysError},
};

use super::{SpaceGuard, UserAccessTrace, UserData, UserDataMut, UserType};

impl SpaceGuard {
    pub fn translated_user_u8(
        &self,
        ptr: *const u8,
    ) -> Result<u8, UniqueSysError<{ SysError::EFAULT as isize }>> {
        let uptr = UserAddr::try_from(ptr)?;
        let user_access_status = &mut local::current_local().user_access_status;
        let value = *uptr.get_mut();
        user_access_status.access_check()?;
        Ok(value)
    }

    pub fn translated_user_array_zero_end<T>(
        &self,
        ptr: *const T,
    ) -> Result<UserData<T>, UniqueSysError<{ SysError::EFAULT as isize }>>
    where
        T: UserType,
    {
        let mut uptr = UserAddr::try_from(ptr)?;
        let user_access_status = &mut local::current_local().user_access_status;
        let _trace = UserAccessTrace::new(user_access_status);
        let mut len = 0;
        let mut get_ch = || {
            let ch: T = unsafe { *uptr.as_ptr() }; // if access fault, return 0.
            uptr.add_assign(1);
            (ch, uptr)
        };
        let (ch, _next_ptr) = get_ch();
        // check first access
        user_access_status.access_check()?;
        if !ch.is_null() {
            len += 1;
        } else {
            let slice = unsafe { &*core::ptr::slice_from_raw_parts(ptr, 0) };
            return Ok(UserData::new(slice));
        }
        loop {
            let (ch, next_ptr) = get_ch();
            if ch.is_null() {
                break;
            }
            len += 1;
            // check when first access a page.
            if next_ptr.page_offset() == core::mem::size_of::<T>() {
                user_access_status.access_check()?;
            }
        }
        user_access_status.access_check()?;
        let slice = unsafe { &*core::ptr::slice_from_raw_parts(ptr, len) };
        return Ok(UserData::new(slice));
    }

    pub fn translated_user_2d_array_zero_end<T>(
        &self,
        ptr: *const *const T,
    ) -> Result<Vec<UserData<T>>, UniqueSysError<{ SysError::EFAULT as isize }>>
    where
        T: UserType,
    {
        let arr_1d = self.translated_user_array_zero_end(ptr)?;
        let mut ret = Vec::new();
        for &arr_2d in &*arr_1d.access(self) {
            ret.push(self.translated_user_array_zero_end(arr_2d)?);
        }
        Ok(ret)
    }

    pub fn translated_user_readonly_slice<T>(
        &self,
        ptr: *const T,
        len: usize,
    ) -> Result<UserData<T>, UniqueSysError<{ SysError::EFAULT as isize }>> {
        let ubegin = UserAddr::try_from(ptr)?;
        let uend = UserAddr::try_from(unsafe { ptr.offset(len as isize) as *mut u8 })?;
        let user_access_status = &mut local::current_local().user_access_status;
        let trace = UserAccessTrace::new(user_access_status);
        let mut cur = ubegin.floor();
        let uend4k = uend.ceil();
        while cur != uend4k {
            let cur_ptr = cur.into_usize() as *const u8;
            // if error occur will change status by exception
            let _v = unsafe { cur_ptr.read_volatile() };
            user_access_status.access_check()?;
            cur.add_page_assign(PageCount::from_usize(1));
        }
        drop(trace);
        let slice = core::ptr::slice_from_raw_parts(ptr, len);
        Ok(UserData::new(unsafe { &*slice }))
    }

    pub fn translated_user_writable_slice<T>(
        &self,
        ptr: *mut T,
        len: usize,
    ) -> Result<UserDataMut<T>, UniqueSysError<{ SysError::EFAULT as isize }>> {
        let ubegin = UserAddr::try_from(ptr)?;
        let uend = UserAddr::try_from(unsafe { ptr.offset(len as isize) as *mut u8 })?;
        let user_access_status = &mut local::current_local().user_access_status;
        let trace = UserAccessTrace::new(user_access_status);
        let mut cur = ubegin.floor();
        let uend4k = uend.ceil();
        while cur != uend4k {
            let cur_ptr = cur.into_usize() as *mut u8;
            unsafe {
                // if error occur will change status by exception
                let v = cur_ptr.read_volatile();
                cur_ptr.write_volatile(v);
            }
            local::current_local().user_access_status.access_check()?;
            cur.add_page_assign(PageCount::from_usize(1));
        }
        drop(trace);
        let slice = core::ptr::slice_from_raw_parts_mut(ptr, len);
        Ok(UserDataMut::new(slice))
    }
}
