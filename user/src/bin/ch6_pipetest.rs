#![no_std]
#![no_main]

#[macro_use]
extern crate user_lib;

use user_lib::{fork, close, pipe, read, write, wait};

static STR: &str = "Hello, world!";

#[no_mangle]
pub fn main() -> i32 {
    // create pipe
    let mut pipe_fd = [0usize; 2];
    pipe(&mut pipe_fd);
    // read end
    assert_eq!(pipe_fd[0], 3);
    // write end
    assert_eq!(pipe_fd[1], 4);
    if fork() == 0 {
        // 子进程会完全继承父进程的文件描述符表
        // 子进程也可以通过同样的文件描述符来访问同一个管道的读端和写端
        // child process, read from parent
        // close write_end, 在子进程中关闭管道的写端
        close(pipe_fd[1]);
        let mut buffer = [0u8; 32];
        // 从管道的读端读取字符串
        let len_read = read(pipe_fd[0], &mut buffer) as usize;
        // close read_end
        close(pipe_fd[0]);
        assert_eq!(core::str::from_utf8(&buffer[..len_read]).unwrap(), STR);
        println!("Read OK, child process exited!");
        0
    } else {
        // parent process, write to child
        // close read end, 在父进程中关闭管道的读端
        close(pipe_fd[0]);
        // 将字符串 STR 写入管道的写端
        assert_eq!(write(pipe_fd[1], STR.as_bytes()), STR.len() as isize);
        // close write end
        close(pipe_fd[1]);
        let mut child_exit_code: i32 = 0;
        wait(&mut child_exit_code);
        assert_eq!(child_exit_code, 0);
        println!("pipetest passed!");
        0
    }
}
