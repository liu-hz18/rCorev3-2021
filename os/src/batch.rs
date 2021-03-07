use core::cell::RefCell;
use lazy_static::*;
use crate::trap::TrapContext;

pub const USER_STACK_SIZE: usize = 4096 * 2; // 8KiB 栈
const KERNEL_STACK_SIZE: usize = 4096 * 2; // 8KiB 栈
const MAX_APP_NUM: usize = 16;
pub const APP_BASE_ADDRESS: usize = 0x80400000;
const APP_SIZE_LIMIT: usize = 0x20000;

// 在批处理操作系统中加入一段汇编代码，实现从用户栈切换到内核栈， 并在内核栈上保存应用程序执行流的寄存器状态。
// 只是字节数组的简单包装
#[repr(align(4096))]
struct KernelStack {
    data: [u8; KERNEL_STACK_SIZE],
}

#[repr(align(4096))]
struct UserStack {
    data: [u8; USER_STACK_SIZE],
}

static KERNEL_STACK: KernelStack = KernelStack { data: [0; KERNEL_STACK_SIZE] };
static USER_STACK: UserStack = UserStack { data: [0; USER_STACK_SIZE] };

impl KernelStack {
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + KERNEL_STACK_SIZE
    }
    pub fn push_context(&self, cx: TrapContext) -> &'static mut TrapContext {
        let cx_ptr = (self.get_sp() - core::mem::size_of::<TrapContext>()) as *mut TrapContext;
        unsafe { *cx_ptr = cx; }
        unsafe { cx_ptr.as_mut().unwrap() }
    }
}

impl UserStack {
    // 换栈是非常简单的，只需将 sp 寄存器的值修改为 get_sp 的返回值即可
    fn get_sp(&self) -> usize {
        self.data.as_ptr() as usize + USER_STACK_SIZE
    }
}

// 找到并加载应用程序二进制码的应用管理器
// NOTE: 利用 RefCell 来提供 `内部可变性`
//       我们希望将 AppManager 实例化为一个全局变量使得 任何函数都可以直接访问
//       但是里面的 current_app 字段表示当前执行到了第几个应用，它会在系统运行期间发生变化
//       所以使用智能指针来绕过安全检查
struct AppManager {
    inner: RefCell<AppManagerInner>,
}

// 保存应用数量和各自的位置信息，以及当前执行到第几个应用了。
// 根据应用程序位置信息，初始化好应用所需内存空间，并加载应用执行。
struct AppManagerInner {
    num_app: usize,
    current_app: usize,
    app_start: [usize; MAX_APP_NUM + 1],
}

// 为了让 AppManager 能被直接全局实例化，我们需要将其标记为 Sync
unsafe impl Sync for AppManager {}

impl AppManagerInner {
    pub fn print_app_info(&self) {
        println!("[kernel] num_app = {}", self.num_app);
        for i in 0..self.num_app {
            println!("[kernel] app_{} [{:#x}, {:#x}) -> [{:#x}, {:#x})", i, self.app_start[i], self.app_start[i + 1], APP_BASE_ADDRESS, APP_BASE_ADDRESS+self.app_start[i + 1]-self.app_start[i]);
        }
    }

    unsafe fn load_app(&self, app_id: usize) {
        if app_id >= self.num_app {
            panic!("All applications completed!");
        }
        println!("[kernel] Loading app_{}", app_id);
        // clear icache
        // 在取指 的时候，对于一个指令地址， CPU 会先去 i-cache 里面看一下它是否在某个已缓存的缓存行内
        // 如果在的话它就会直接从高速缓存中拿到指令而不是通过 总线和内存通信
        llvm_asm!("fence.i" :::: "volatile");
        // clear app area
        (APP_BASE_ADDRESS..APP_BASE_ADDRESS + APP_SIZE_LIMIT).for_each(|addr| {
            (addr as *mut u8).write_volatile(0);
        });
        let app_src = core::slice::from_raw_parts(
            self.app_start[app_id] as *const u8,
            self.app_start[app_id + 1] - self.app_start[app_id]
        );
        // 二进制镜像加载到物理内存以 0x80040000 开头的位置
        // 这个位置是批处理操作系统和应用程序 之间约定的常数地址
        let app_dst = core::slice::from_raw_parts_mut(
            APP_BASE_ADDRESS as *mut u8,
            app_src.len()
        );
        app_dst.copy_from_slice(app_src);
    }

    pub fn get_current_app(&self) -> usize { self.current_app }

    pub fn get_current_app_runtime_end(&self) -> usize { APP_BASE_ADDRESS + self.app_start[self.current_app + 1] - self.app_start[self.current_app] }

    pub fn move_to_next_app(&mut self) {
        self.current_app += 1;
    }
}

// 初始化 AppManager 的全局实例
lazy_static! {
    static ref APP_MANAGER: AppManager = AppManager {
        inner: RefCell::new({
            // 找到 link_app.S 中提供的符号 _num_app
            extern "C" { fn _num_app(); }
            // 从这里开始解析出应用数量以及各个应用的开头地址
            let num_app_ptr = _num_app as usize as *const usize;
            let num_app = unsafe { num_app_ptr.read_volatile() };
            let mut app_start: [usize; MAX_APP_NUM + 1] = [0; MAX_APP_NUM + 1];
            let app_start_raw: &[usize] = unsafe {
                core::slice::from_raw_parts(num_app_ptr.add(1), num_app + 1)
            };
            app_start[..=num_app].copy_from_slice(app_start_raw);
            AppManagerInner {
                num_app,
                current_app: 0,
                app_start,
            }
        }),
    };
}

pub fn init() {
    // 打印 UserStack 和Kernel Stack 地址范围
    // 8KiB 栈
    println!("[kernel] Kernel Stack [{:#x}, {:#x})", KERNEL_STACK.data.as_ptr() as usize, KERNEL_STACK.data.as_ptr() as usize + KERNEL_STACK_SIZE);
    println!("[kernel] User   Stack [{:#x}, {:#x})", USER_STACK.data.as_ptr() as usize, USER_STACK.data.as_ptr() as usize + USER_STACK_SIZE);
    // 调用 print_app_info 的时候第一次用到了全局变量 APP_MANAGER ，它也是在这个时候完成初始化
    print_app_info();
}

pub fn print_app_info() {
    APP_MANAGER.inner.borrow().print_app_info();
}

pub fn get_current_app_runtime_end() -> usize {
    APP_MANAGER.inner.borrow().get_current_app_runtime_end()
}

pub fn get_current_app() -> usize {
    APP_MANAGER.inner.borrow().get_current_app()
}

pub fn addr_in_user_stack(addr: usize) -> bool {
    addr > USER_STACK.data.as_ptr() as usize && addr < USER_STACK.data.as_ptr() as usize + USER_STACK_SIZE
}

// 批处理操作系统的核心操作，即加载并运行下一个应用程序
// 在运行应用程序之前要完成如下这些工作:
// 1. 跳转到应用程序入口点 0x80040000 (by push_context)
// 2. 将使用的栈切换到用户栈 (by __restore)
// 3. 在 __alltraps 时我们要求 sscratch 指向内核栈，这个也需要在此时完成 (by __restore)
// 4. 从 S 特权级切换到 U 特权级 (by app_init_context)
// 我们只需要在内核栈上压入一个相应构造的 Trap 上下文，再通过 __restore ，就能 让这些寄存器到达我们希望的状态。
pub fn run_next_app() -> ! {
    let current_app = APP_MANAGER.inner.borrow().get_current_app();
    unsafe {
        APP_MANAGER.inner.borrow().load_app(current_app);
    }
    APP_MANAGER.inner.borrow_mut().move_to_next_app();
    extern "C" { fn __restore(cx_addr: usize); }
    unsafe {
        // 在内核栈上压入一个 Trap 上下文，其 sepc 是应用程序入口地址 0x80040000, 其 sp 寄存器指向用户栈
        // push_context 的返回值是内核栈压入 Trap 上下文之后的栈顶，它会被作为 __restore 的参数
        // 使得在 __restore 中 sp 仍然可以指向内核栈的栈顶
        __restore(KERNEL_STACK.push_context(
            TrapContext::app_init_context(APP_BASE_ADDRESS, USER_STACK.get_sp())
        ) as *const _ as usize);
    }
    panic!("Unreachable in batch::run_current_app!");
}
