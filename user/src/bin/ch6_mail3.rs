#![no_std]
#![no_main]

extern crate core;
#[macro_use]
extern crate user_lib;

use user_lib::{getpid, mail_read, mail_write};
use core::slice;

const BUF_LEN: usize = 256;
const MAIL_MAX: usize = 16;

#[no_mangle]
fn main() -> i32 {
    let pid = getpid();
    let null = unsafe { slice::from_raw_parts(0x0 as *const _, 10) };
    assert_eq!(mail_write(pid as usize, null), -1); // buf不合法
    let mut empty = ['a' as u8; 0];
    assert_eq!(mail_write(pid as usize, &empty), 0); // 写入0字节，邮箱可写，返回0
    assert_eq!(mail_read(&mut empty), -1); // 邮箱空，返回-1
    let buffer0 = ['a' as u8; BUF_LEN];
    for _ in 0..MAIL_MAX {
        assert_eq!(mail_write(pid as usize, &buffer0), BUF_LEN as isize);
    }
    assert_eq!(mail_write(pid as usize, &empty), -1); // 邮箱满
    assert_eq!(mail_read(&mut empty), 0); // 可读，读取0字节，返回0
    assert_eq!(mail_write(pid as usize, &empty), -1); // 邮箱满
    println!("mail3 test OK!");
    0
}