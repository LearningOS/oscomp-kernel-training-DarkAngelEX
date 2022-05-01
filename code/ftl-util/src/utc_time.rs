pub struct UtcTime {
    pub ymd: (usize, usize, usize),
    pub hms: (usize, usize, usize),
    pub ms: usize,
}

impl UtcTime {
    pub fn base() -> Self {
        let mut v: Self = unsafe { core::mem::MaybeUninit::zeroed().assume_init() };
        v.ymd.0 = 1980;
        v
    }
}
