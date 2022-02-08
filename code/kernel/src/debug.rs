pub const PRINT_MAP_ALL: bool = false;

pub const _DEBUG: bool = true;

#[macro_export]
macro_rules! debug_run {
    () => {};
    ($x: expr) => {
        if crate::debug::_DEBUG {
            $x;
        }
    };
}

#[macro_export]
macro_rules! debug_check {
    ($($arg:tt)*) => {
        if crate::debug::_DEBUG { assert!($($arg)*); }
    }
}

#[macro_export]
macro_rules! debug_check_eq {
    ($($arg:tt)*) => {
        if crate::debug::_DEBUG { assert_eq!($($arg)*); }
    }
}
