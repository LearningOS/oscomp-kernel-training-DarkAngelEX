#[derive(Clone, Copy)]
pub struct AuxHeader {
    pub aux_type: usize,
    pub value: usize,
}

impl AuxHeader {
    pub fn write_to(self, dst: &mut [usize; 2]) {
        dst[0] = self.aux_type;
        dst[1] = self.value;
    }
}