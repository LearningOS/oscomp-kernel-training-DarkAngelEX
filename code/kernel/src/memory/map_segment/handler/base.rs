/// 实现一些页表的公共设施
#[derive(Clone)]
pub struct HandlerBase {}

impl HandlerBase {
    pub const fn new() -> Self {
        Self {}
    }
}

impl const Default for HandlerBase {
    fn default() -> Self {
        Self::new()
    }
}
