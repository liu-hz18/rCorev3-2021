use crate::trap::TrapContext;
use crate::task::TaskContext;
use crate::config::*;

#[repr(align(4096))]
struct KernelStack {
    data: [u8; KERNEL_STACK_SIZE],
}

#[repr(align(4096))]
struct UserStack {
    data: [u8; USER_STACK_SIZE],
}

// 每个应用程序都有自己单独的 内核栈 和 用户栈
static KERNEL_STACK: [KernelStack; MAX_APP_NUM] = [
    KernelStack { data: [0; KERNEL_STACK_SIZE], };
    MAX_APP_NUM
];

static USER_STACK: [UserStack; MAX_APP_NUM] = [
    UserStack { data: [0; USER_STACK_SIZE], };
    MAX_APP_NUM
];

impl KernelStack {
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + KERNEL_STACK_SIZE
    }
    pub fn push_context(&self, trap_cx: TrapContext, task_cx: TaskContext) -> &'static mut TaskContext {
        unsafe {
            // 先压入一个和之前相同的 Trap 上下文
            let trap_cx_ptr = (self.get_sp() - core::mem::size_of::<TrapContext>()) as *mut TrapContext;
            *trap_cx_ptr = trap_cx;
            // 再在它上面压入一个任务上下文，
            let task_cx_ptr = (trap_cx_ptr as usize - core::mem::size_of::<TaskContext>()) as *mut TaskContext;
            *task_cx_ptr = task_cx;
            // 返回任务上下文的地址
            task_cx_ptr.as_mut().unwrap()
        }
    }
}

impl UserStack {
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + USER_STACK_SIZE
    }
}

fn get_base_i(app_id: usize) -> usize {
    APP_BASE_ADDRESS + app_id * APP_SIZE_LIMIT
}

pub fn get_num_app() -> usize {
    extern "C" { fn _num_app(); }
    unsafe { (_num_app as usize as *const usize).read_volatile() }
}

// 所有的应用在内核初始化的时候就一并被加载到内存中
// 为了避免覆盖，它们自然需要被加载到不同的物理地址
// 从 APP_BASE_ADDRESS 开始依次为每个应用预留一段空间
pub fn load_apps() {
    extern "C" { fn _num_app(); }
    let num_app_ptr = _num_app as usize as *const usize;
    let num_app = get_num_app();
    let app_start = unsafe {
        core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1)
    };
    // clear i-cache first
    unsafe { llvm_asm!("fence.i" :::: "volatile"); }
    // load apps
    for i in 0..num_app {
        // 第 i 个应用被加载到以物理地址 base_i 开头的一段物理内存上
        let base_i = get_base_i(i);
        // clear region
        (base_i..base_i + APP_SIZE_LIMIT).for_each(|addr| unsafe {
            (addr as *mut u8).write_volatile(0)
        });
        // load app from data section to memory
        let src = unsafe {
            core::slice::from_raw_parts(app_start[i] as *const u8, app_start[i + 1] - app_start[i])
        };
        let dst = unsafe {
            core::slice::from_raw_parts_mut(base_i as *mut u8, src.len())
        };
        dst.copy_from_slice(src);
        println!("[kernel] app_{} mem: [{:#x}, {:#x}) -> [{:#x}, {:#x})", i, app_start[i], app_start[i+1], base_i, base_i + src.len());
        println!("               kernel stack: [{:#x}, {:#x})", KERNEL_STACK[i].data.as_ptr() as usize, KERNEL_STACK[i].data.as_ptr() as usize + KERNEL_STACK_SIZE);
        println!("               user stack:   [{:#x}, {:#x})", USER_STACK[i].data.as_ptr() as usize, USER_STACK[i].data.as_ptr() as usize + USER_STACK_SIZE);
    }
}

pub fn init_app_cx(app_id: usize) -> &'static TaskContext {
    KERNEL_STACK[app_id].push_context(
        TrapContext::app_init_context(get_base_i(app_id), USER_STACK[app_id].get_sp()),
        TaskContext::goto_restore(), // 构造 任务上下文
    )
}
