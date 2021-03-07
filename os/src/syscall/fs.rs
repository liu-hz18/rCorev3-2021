const FD_STDOUT: usize = 1;

use crate::batch::{APP_BASE_ADDRESS, addr_in_user_stack, get_current_app_runtime_end, get_current_app, run_next_app};

// 安全检查：sys_write 仅能输出位于程序本身内存空间内的数据，否则报错
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        // 注意这里我们并没有检查传入参数的安全性，即使会在出错严重的时候 panic，还是会存在安全隐患。
        FD_STDOUT => {
            let buf_usize: usize = buf as usize;
            // check if in user stack
            if addr_in_user_stack(buf_usize) {
                println!("[kernel] string buffer overflow the region of USER Stack, in sys_write()");
                run_next_app();
            }
            // check if in user program code region
            if (buf_usize < APP_BASE_ADDRESS) || (buf_usize > get_current_app_runtime_end()) {
                println!("[kernel] string buffer overflow the region of APP {}, in sys_write()", get_current_app());
                run_next_app();
            }
            let slice = unsafe { core::slice::from_raw_parts(buf, len) };
            let str = core::str::from_utf8(slice).unwrap();
            print!("{}", str);
            len as isize
        },
        _ => {
            panic!("[kernel] Unsupported fd in sys_write!");
        }
    }
}
