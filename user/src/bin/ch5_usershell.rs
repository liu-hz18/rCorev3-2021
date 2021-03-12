#![no_std]
#![no_main]

extern crate alloc;

#[macro_use]
extern crate user_lib;

const LF: u8 = 0x0au8;
const CR: u8 = 0x0du8;
const DL: u8 = 0x7fu8;
const BS: u8 = 0x08u8;

use alloc::string::String;
use user_lib::console::getchar;
use user_lib::{spawn, waitpid, yield_};

#[no_mangle]
pub fn main() -> i32 {
    println!("Rust user shell");
    let mut line: String = String::new();
    print!(">> ");
    loop {
        let c = getchar();
        match c {
            // 回车键, fork 出一个子进程并试图通过 exec 系统调用执行一个应用
            // 如果返回值为 -1 的话目前说明在应用管理器中找不到名字相同的应用，此时子进程就直接打印错误信息并退出
            LF | CR => {
                println!("");
                if !line.is_empty() {
                    line.push('\0');
                    let cpid = spawn(line.as_str());
                    if cpid < 0 {
                        println!("invalid file name {}", line.as_str());
                        line.clear();
                        print!(">> ");
                        continue;
                    }
                    let mut xstate: i32 = 0;
                    let mut exit_pid: isize;
                    loop {
                        exit_pid = waitpid(cpid as usize, &mut xstate);
                        println!("exit_pid: {}", exit_pid);
                        if exit_pid == -1 {
                            yield_();
                        } else {
                            assert_eq!(cpid, exit_pid);
                            println!("Shell: Process {} exited with code {}", cpid, xstate);
                            break;
                        }
                    }
                    line.clear();
                }
                print!(">> ");
            }
            // 输入退格键（第 53 行），首先我们需要将屏幕上当前行的最后一个字符用空格替换掉，这可以通过输入一个特殊的退格字节 BS 来实现
            BS | DL => {
                if !line.is_empty() {
                    print!("{}", BS as char);
                    print!(" ");
                    print!("{}", BS as char);
                    line.pop(); // line 也需要弹出最后一个字符
                }
            }
            _ => {
                print!("{}", c as char);
                line.push(c as char);
            }
        }
    }
}
