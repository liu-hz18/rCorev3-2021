
use riscv::register::time;
use crate::sbi::set_timer; // 由 SEE 提供的标准 SBI 接口函数，它可以用来设置 mtimecmp 的值
use crate::config::CLOCK_FREQ;

const TICKS_PER_SEC: usize = 100;
const MSEC_PER_SEC: usize = 1000;

pub fn get_time() -> usize {
    time::read()
}

// 计时, 以 毫秒 为单位返回当前计数器的值
pub fn get_time_ms() -> usize {
    time::read() / (CLOCK_FREQ / MSEC_PER_SEC)
}

// 设置 10ms 的计时器
pub fn set_next_trigger() {
    // 对 set_timer 进行了封装，它首先读取 当前 mtime 的值，
    // 然后计算出 10ms 之内计数器的增量，再将 mtimecmp 设置为二者的和。
    // 10ms 之后 一个 S 特权级时钟中断就会被触发
    set_timer(get_time() + CLOCK_FREQ / TICKS_PER_SEC);
}
