// use crate::futex::FutexSet;

/// 被迫继承
#[derive(Clone)]
pub struct HandlerBase {
    // futex: FutexSet,
}

impl HandlerBase {
    pub fn new() -> Self {
        Self {
            // futex: FutexSet::new(),
        }
    }
}
