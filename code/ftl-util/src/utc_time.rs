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
    pub fn set_ymd(&mut self, ymd: u16) {
        let year = (ymd as usize >> 9) + 1980;
        let mount = (ymd as usize) >> 5 & ((1 << 4) - 1);
        let day = ymd as usize & ((1 << 5) - 1);
        self.ymd = (year, mount, day);
    }
    pub fn set_hms(&mut self, hms: u16) {
        let hour = (hms as usize >> 11).min(23);
        let minute = ((hms as usize) >> 5 & ((1 << 6) - 1)).min(59);
        let second = ((hms as usize & ((1 << 5) - 1)) * 2).min(59);
        self.hms = (hour, minute, second);
    }
    pub fn set_ms(&mut self, ms: u8) {
        self.ms = ms as usize * 10;
    }
    pub fn second(&self) -> usize {
        let mut cur = (self.ymd.0 - 1980) * 365 * 24 * 3600;
        cur += self.ymd.1 * 30 * 24 * 3600;
        cur += self.ymd.2 * 24 * 3600;
        cur += self.hms.0 * 3600;
        cur += self.hms.1 * 60;
        cur += self.hms.2;
        cur
    }
    pub fn nanosecond(&self) -> usize {
        self.ms * 1000 * 1000
    }
}
