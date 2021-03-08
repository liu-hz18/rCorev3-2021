const FD_STDOUT: usize = 1;

use crate::batch::{APP_BASE_ADDRESS, addr_in_user_stack, get_current_app_runtime_end, get_current_app};

fn addr_in_app(addr: usize) -> bool {
    (addr >= APP_BASE_ADDRESS) && (addr < get_current_app_runtime_end())
}

fn addr_range_valid(buf: usize, len: usize) -> bool {
    (addr_in_user_stack(buf) && addr_in_user_stack(buf + len)) || (addr_in_app(buf) && addr_in_app(buf + len))
}

// 安全检查：sys_write 仅能输出位于程序本身内存空间内的数据，否则报错
pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    match fd {
        // 注意这里我们并没有检查传入参数的安全性，即使会在出错严重的时候 panic，还是会存在安全隐患。
        FD_STDOUT => {
            // check if in user stack
            // check if in .text .data .bss
            if !addr_range_valid(buf as usize, len) {
                println!("[kernel] buffer overflow in APP {}, in sys_write! addr=[{:#x}, {:#x})", get_current_app()-1, buf as usize, buf as usize + len);
                -1 as isize
            } else {
                let slice = unsafe { core::slice::from_raw_parts(buf, len) };
                let str = core::str::from_utf8(slice).unwrap();
                print!("{}", str);
                len as isize
            }
        },
        _ => {
            println!("[kernel] Unsupported fd in sys_write!");
            return -1 as isize;
        }
    }
}
