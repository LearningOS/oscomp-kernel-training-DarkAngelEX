/// 这个函数比copy_from_slice快!
#[inline(never)]
pub fn page_copy(dst: &mut [usize; 512], src: &[usize; 512]) {
    for i in 0..64 {
        dst[i * 8 + 0] = src[i * 8 + 0];
        dst[i * 8 + 1] = src[i * 8 + 1];
        dst[i * 8 + 2] = src[i * 8 + 2];
        dst[i * 8 + 3] = src[i * 8 + 3];
        dst[i * 8 + 4] = src[i * 8 + 4];
        dst[i * 8 + 5] = src[i * 8 + 5];
        dst[i * 8 + 6] = src[i * 8 + 6];
        dst[i * 8 + 7] = src[i * 8 + 7];
    }
}

#[inline(never)]
pub fn huge_copy(mut dst: &mut [usize], mut src: &[usize]) {
    assert!(dst.len() == src.len());
    assert!(dst.len() >= 512);
    let mut n = dst.len();
    // 对齐到cache_line
    unsafe {
        loop {
            page_copy(
                core::mem::transmute(dst.as_mut_ptr()),
                core::mem::transmute(src.as_ptr()),
            );
            dst = &mut dst[512..];
            src = &src[512..];
            n -= 512;
            if n < 512 {
                break;
            }
        }
        if n != 0 {
            dst.copy_from_slice(src);
        }
        return;
    }
}

#[inline(always)]
pub fn u8copy(dst: &mut [u8], src: &[u8]) {
    #[inline(never)]
    #[cold]
    fn u8_fail() -> ! {
        panic!("u8 copy len check fail");
    }
    if dst.len() != src.len() {
        u8_fail();
    }
    const USIZE_SIZE: usize = core::mem::size_of::<usize>();
    let len = dst.len();
    unsafe {
        if len < 4096
            || dst.as_ptr() as usize % USIZE_SIZE != 0
            || src.as_ptr() as usize % USIZE_SIZE != 0
        {
            return core::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), len);
        }
        huge_copy(
            core::slice::from_raw_parts_mut(dst.as_mut_ptr() as _, len / USIZE_SIZE),
            core::slice::from_raw_parts(src.as_ptr() as _, len / USIZE_SIZE),
        )
    }
}
