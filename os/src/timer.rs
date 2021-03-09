
use riscv::register::time;
use crate::sbi::set_timer; // 由 SEE 提供的标准 SBI 接口函数，它可以用来设置 mtimecmp 的值
use crate::config::CLOCK_FREQ;

const TICKS_PER_SEC: usize = 100;
const MSEC_PER_SEC: usize = 1000;
const USEC_PER_SEC: usize = 1000000;

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct TimeVal {
    pub sec: usize, // seconds
    pub usec: usize, // microseconds
}

pub fn get_time() -> usize {
    time::read()
}

// 计时, 以 毫秒 为单位返回当前计数器的值
pub fn get_time_ms() -> usize {
    time::read() / (CLOCK_FREQ / MSEC_PER_SEC)
}

// CLOCK_FREQ / USEC_PER_SEC == 12.5
// 1 / (CLOCK_FREQ / USEC_PER_SEC) == 0.08 == 2 / 25
pub fn get_time_us() -> usize {
    time::read() * 2 / 25
}

// tz 表示时区，这里无需考虑，始终为0
// ts 为当前时间结构体
// 正确返回 0，错误返回 -1
pub fn get_time_sys(ts: *mut TimeVal, _tz: usize) -> isize {
    unsafe {
        if let Some(ts) = ts.as_mut() {
            (*ts).usec = get_time_us() % USEC_PER_SEC;
            (*ts).sec = get_time_us() / USEC_PER_SEC;
            return 0
        }
    }
    -1
}

// 设置 10ms 的计时器
pub fn set_next_trigger() {
    // 对 set_timer 进行了封装，它首先读取 当前 mtime 的值，
    // 然后计算出 10ms 之内计数器的增量，再将 mtimecmp 设置为二者的和。
    // 10ms 之后 一个 S 特权级时钟中断就会被触发
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}
