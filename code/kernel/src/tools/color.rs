#[macro_export]
macro_rules! color_str {
    ($n: expr) => {
        concat!("\x1b[", $n, "m")
    };
}
#[macro_export]
macro_rules! reset_color {
    () => {
        color_str!(0)
    };
}
#[macro_export]
macro_rules! to_red {
    () => {
        color_str!(31)
    };
    ($str: literal) => {
        concat!(to_red!(), $str, reset_color!())
    };
}
#[macro_export]
macro_rules! to_green {
    () => {
        color_str!(32)
    };
    ($str: literal) => {
        concat!(to_green!(), $str, reset_color!())
    };
}
#[macro_export]
macro_rules! to_blue {
    () => {
        color_str!(34)
    };
    ($str: literal) => {
        concat!(to_blue!(), $str, reset_color!())
    };
}
#[macro_export]
macro_rules! to_gray {
    () => {
        color_str!(90)
    };
    ($str: literal) => {
        concat!(to_gray!(), $str, reset_color!())
    };
}
#[macro_export]
macro_rules! to_yellow {
    () => {
        color_str!(93)
    };
    ($str: literal) => {
        concat!(to_yellow!(), $str, reset_color!())
    };
}

pub mod test {
    pub fn color_test() {
        println!("color_test begin");
        for i in 0..30 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!();
        for i in 30..60 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!();
        for i in 60..90 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!();
        for i in 90..120 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!();
        println!("color_test end");
        panic!()
    }
}
