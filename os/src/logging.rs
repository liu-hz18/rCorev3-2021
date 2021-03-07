use core::fmt;
use core::option_env;
use log::{self, Level, LevelFilter, Log, Metadata, Record};
use crate::console::print;

// print with color!
// e.g.: \x1b[31mhello world\x1b[0m
// ERROR: red, 31
// WARN: yellow, 93
// INFO: blue, 34
// DEBUG: green, 32
// TRACE: grey, 90
// level: ERROR > WARN > INFO > DEBUG > TRACE
pub fn init() {
    static LOGGER: SimpleLogger = SimpleLogger;
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(match option_env!("LOG") {
        Some("ERROR") => LevelFilter::Error,
        Some("WARN") => LevelFilter::Warn,
        Some("INFO") => LevelFilter::Info,
        Some("DEBUG") => LevelFilter::Debug,
        Some("TRACE") => LevelFilter::Trace,
        Some("OFF") => LevelFilter::Off,
        Some(_level) => {
            // println!("\x1b[93m[LOGGER][0] logging level {:?} is not supported. use default level: `INFO`\x1b[0m", level);
            LevelFilter::Info
        },
        None => {
            // println!("\x1b[93m[LOGGER][0] logging level is not specified. use default level: `INFO`\x1b[0m");
            LevelFilter::Info
        }, // default is INFO
    });
    // println!("\x1b[34m[INFO][0] Logging Level: {:?}\x1b[0m", log::max_level());
}

/// Add escape sequence to print with color in Linux console
macro_rules! with_color {
    ($args: ident, $color_code: ident) => {{
        format_args!("\u{1B}[{}m{}\u{1B}[0m", $color_code as u8, $args)
    }};
}

fn print_in_color(args: fmt::Arguments, color_code: u8) {
    print(with_color!(args, color_code));
}

struct SimpleLogger;

impl Log for SimpleLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= log::max_level()
    }
    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        print_in_color(
            format_args!(
                "[{}][{}] {}\n",
                record.level(),
                0, // cpu id
                record.args()
            ),
            level_to_color_code(record.level()),
        );
    }
    fn flush(&self) {}
}

fn level_to_color_code(level: Level) -> u8 {
    match level {
        Level::Error => 31, // Red
        Level::Warn => 93,  // Yellow
        Level::Info => 34,  // Blue
        Level::Debug => 32, // Green
        Level::Trace => 90, // Grey
    }
}

/// 类似 `std::dbg` 宏
/// 可以实现方便的对变量输出的效果
#[macro_export]
macro_rules! dbg {
    () => {
        println!("\x1b[32m[{}:{}]\x1b[0m", file!(), line!());
    };
    ($val:expr) => {
        match $val {
            tmp => {
                println!("\x1b[32m[{}:{}]\x1b[0m {} = {:#?}",
                    file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($val:expr,) => { $crate::dbg!($val) };
    ($($val:expr),+ $(,)?) => {
        ($($crate::dbg!($val)),+,)
    };
}

#[macro_export]
macro_rules! dbgx {
    () => {
        println!("\x1b[32m[{}:{}]\x1b[0m", file!(), line!());
    };
    ($val:expr) => {
        match $val {
            tmp => {
                println!("\x1b[32m[{}:{}]\x1b[0m {} = {:#x?}",
                    file!(), line!(), stringify!($val), &tmp);
                tmp
            }
        }
    };
    ($val:expr,) => { dbgx!($val) };
    ($($val:expr),+ $(,)?) => {
        ($(dbgx!($val)),+,)
    };
}
