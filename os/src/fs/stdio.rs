use super::File;
use crate::mm::{UserBuffer};
use crate::sbi::console_getchar;
use crate::task::suspend_current_and_run_next;

pub struct Stdin;

pub struct Stdout;

// 实现 File Trait 规定的接口
impl File for Stdin {
    fn read(&self, mut user_buf: UserBuffer) -> usize {
        // 目前每次仅支持读入一个字符
        assert_eq!(user_buf.len(), 1);
        // busy loop
        let mut c: usize;
        loop {
            c = console_getchar();
            if c == 0 {
                suspend_current_and_run_next();
                continue;
            } else {
                break;
            }
        }
        let ch = c as u8;
        // 需要通过 UserBuffer 来获取具体将字节写入的位置
        unsafe { user_buf.buffers[0].as_mut_ptr().write_volatile(ch); }
        1
    }
    fn write(&self, _user_buf: UserBuffer) -> usize {
        panic!("Cannot write to stdin!");
    }
}

impl File for Stdout {
    fn read(&self, _user_buf: UserBuffer) -> usize{
        panic!("Cannot read from stdout!");
    }
    fn write(&self, user_buf: UserBuffer) -> usize {
        // 遍历每个切片，将其转化为字符串通过 print! 宏来输出
        // 如果有多核同时使用 print! 宏，将会导致两个不同的输出交错到一起造成输出混乱
        for buffer in user_buf.buffers.iter() {
            print!("{}", core::str::from_utf8(*buffer).unwrap());
        }
        user_buf.len()
    }
}
