macro_rules! color_def {
    ($name: ident, $n: expr) => {
        #[macro_export]
        macro_rules! $name {
            ($str: expr) => {
                concat!("\x1b[", $n, "m", $str, "\x1b[0m")
            };
        }
    };
}

color_def!(red_str, 31);
color_def!(green_str, 32);
color_def!(blue_str, 34);
color_def!(gray_str, 90);
color_def!(yellow_str, 93);

pub mod test {
    pub fn color_test() {
        println!("color_test begin");
        for i in 0..30 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!("");
        for i in 30..60 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!("");
        for i in 60..90 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!("");
        for i in 90..120 {
            print!("\x1b[{}m{:0>3}\x1b[0m ", i, i);
        }
        println!("");
        println!("color_test end");
        panic!()
    }
}
