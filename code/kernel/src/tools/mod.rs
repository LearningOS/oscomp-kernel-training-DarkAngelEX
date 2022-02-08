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
