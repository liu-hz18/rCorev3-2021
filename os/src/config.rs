pub const USER_STACK_SIZE: usize = 4096;
pub const KERNEL_STACK_SIZE: usize = 4096 * 2;
pub const MAX_APP_NUM: usize = 50;
pub const APP_BASE_ADDRESS: usize = 0x80400000;
pub const APP_SIZE_LIMIT: usize = 0x20000; // 每个应用二进制镜像的大小限制

pub const CLOCK_FREQ: usize = 12500000;

// Stride 调度
pub const BIG_STRIDE: isize = 0x7FFFFFFF;
pub const TASK_INIT_PRIORITY: isize = 16;

// 防止死循环
pub const MAX_EXECUTE_TIME_MS: usize = 20_000;
