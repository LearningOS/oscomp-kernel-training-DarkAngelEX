pub mod allocator;
pub mod container;

pub mod error;

pub const fn bool_result(x: bool) -> Result<(), ()> {
    if x {
        Ok(())
    } else {
        Err(())
    }
}

#[macro_export]
macro_rules! impl_usize_from {
    ($name: ident, $v: ident, $body: stmt) => {
        impl From<$name> for usize {
            fn from($v: $name) -> Self {
                $body
            }
        }
        impl $name {
            pub const fn into_usize(&self) -> usize {
                let $v = self;
                $body
            }
        }
    };
}

pub struct FailRun<T: FnOnce()> {
    drop_run: Option<T>,
}

impl<T: FnOnce()> Drop for FailRun<T> {
    fn drop(&mut self) {
        if let Some(f) = self.drop_run.take() {
            f()
        }
    }
}

impl<T: FnOnce()> FailRun<T> {
    pub fn new(f: T) -> Self {
        Self { drop_run: Some(f) }
    }
    pub fn consume(mut self) {
        self.drop_run = None;
    }
}
