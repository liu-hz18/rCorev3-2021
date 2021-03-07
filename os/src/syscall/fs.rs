const FD_STDOUT: usize = 1;


// 安全检查：sys_write 仅能输出位于程序本身内存空间内的数据，否则报错
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        // 注意这里我们并没有检查传入参数的安全性，即使会在出错严重的时候 panic，还是会存在安全隐患。
        FD_STDOUT => {
            let slice = unsafe { core::slice::from_raw_parts(buf, len) };
            let str = core::str::from_utf8(slice).unwrap();
            print!("{}", str);
            len as isize
        },
        _ => {
            panic!("Unsupported fd in sys_write!");
        }
    }
}
