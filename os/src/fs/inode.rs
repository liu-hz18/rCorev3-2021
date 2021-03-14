// 内核索引节点层
use easy_fs::{
    EasyFileSystem,
    Inode,
};
use crate::drivers::BLOCK_DEVICE;
use lazy_static::*;
use bitflags::*;
use spin::Mutex;
use super::File;
use crate::mm::UserBuffer;
use alloc::vec::Vec;
use alloc::sync::Arc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::prelude::v1::Box;
use alloc::rc::Rc;

// 硬链接映射表:
lazy_static! {
    pub static ref HARD_LINK_MAP: Mutex<BTreeMap<String, Arc<OSInode>>> = Mutex::new(BTreeMap::new());
}

pub fn link(old_path_str: &str, new_path_str: &str) -> isize {
    if old_path_str == new_path_str {
        return -1;
    }
    let old_path = String::from(old_path_str);
    let new_path = String::from(new_path_str);
    // 处理创建硬链接的硬链接
    let mut map_lock = HARD_LINK_MAP.lock();
    if let Some(old_inode) = map_lock.get(&old_path) {
        old_inode.inner.lock().nlink.lock().0 += 1;
        let new_inode = Arc::clone(old_inode);
        map_lock.insert(new_path, new_inode);
        0
    } else {
        -1
    }
}

pub fn unlink(path_str: &str) -> isize {
    let path = String::from(path_str);
    let mut map_lock = HARD_LINK_MAP.lock();
    let mut ret_value: isize = 0;
    let mut only_one_link = false;
    if let Some(old_inode) = map_lock.get(&path) {
        let mut inner = old_inode.inner.lock();
        let mut inner_nlink = inner.nlink.lock();
        if inner_nlink.0 > 1 {
            inner_nlink.0 -= 1;
            only_one_link = inner_nlink.0 == 1;
            ret_value = 0;
        } else {
            ret_value = -1;
        }
    } else {
        ret_value = -1;
    }
    if ret_value == 0 && only_one_link {
        map_lock.remove(&path);
    }
    ret_value
}

pub fn map(path_str: String, inode: Arc<OSInode>) {
    // insert and update
    HARD_LINK_MAP.lock().insert(path_str, inode);
}

// 只能控制进程对本次打开的文件的访问
// 在我们简化版的文件系统中文件不进行权限设置
// 将一个 u32 的 flags 包装为一个 OpenFlags 结构体更易使用，它的 bits 字段可以将自身转回 u32
// 打开文件的标志
bitflags! {
    pub struct OpenFlags: u32 {
        const RDONLY = 0; // 0, 只读模式 
        const WRONLY = 1 << 0; // 0x001, 只写模式
        const RDWR = 1 << 1; // 0x002, 既可读又可写
        // 在打开文件时 CREATE 标志使得如果 filea 原本不存在，文件系统会自动创建一个同名文件，如果已经存在的话则会清空它的内容
        const CREATE = 1 << 9; // 0x200, 允许创建文件, 在找不到该文件的时候应创建文件; 如果该文件已经存在则应该将该文件的大小归零
        const TRUNC = 1 << 10; // 0x400, 在打开文件的时候应该清空文件的内容并将该文件的大小归零
    }
}

// OS 中的索引节点
// 表示进程中一个被打开的标准文件或目录
pub struct OSInode {
    readable: bool,
    writable: bool,
    pub inner: Mutex<OSInodeInner>,
}

pub struct LinkNumber(pub usize);

pub struct OSInodeInner {
    pub nlink: Arc<Mutex<LinkNumber>>,
    offset: usize, // 在 sys_read/write 期间被维护偏移量
    pub inode: Arc<Inode>,
}

impl OSInode {
    pub fn new(
        readable: bool,
        writable: bool,
        nlink: Arc<Mutex<LinkNumber>>,
        inode: Arc<Inode>,
    ) -> Self {
        Self {
            readable,
            writable,
            inner: Mutex::new(OSInodeInner {
                nlink: nlink, // 硬链接初始为1
                offset: 0,
                inode,
            }),
        }
    }
    // 将该文件的数据全部读到一个 u8 向量 中
    pub fn read_all(&self) -> Vec<u8> {
        let mut inner = self.inner.lock();
        let mut buffer = [0u8; 512];
        let mut v: Vec<u8> = Vec::new();
        loop {
            let len = inner.inode.read_at(inner.offset, &mut buffer);
            if len == 0 {
                break;
            }
            inner.offset += len;
            v.extend_from_slice(&buffer[..len]);
        }
        v
    }
}

// 文件系统初始化
lazy_static! {
    pub static ref ROOT_INODE: Arc<Inode> = {
        // 打开块设备BLOCK_DEVICE, 从块设备 BLOCK_DEVICE 上打开文件系统
        let efs = EasyFileSystem::open(BLOCK_DEVICE.clone());
        // 从文件系统中获取根目录的 inode 
        Arc::new(EasyFileSystem::root_inode(&efs))
    };
}

pub fn list_apps() {
    println!("/**** APPS ****");
    for app in ROOT_INODE.ls() {
        let inode = ROOT_INODE.find(&app[..]).unwrap();
        map(app.clone(), Arc::new(OSInode::new(
            true,
            false,
            Arc::new(Mutex::new(LinkNumber(1 as usize))),
            inode,
        )));
    }
    println!("**************/")
}

impl OpenFlags {
    /// Do not check validity for simplicity
    /// Return (readable, writable)
    // 根据标志的情况返回要打开的文件是否允许读写
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() { // RONLY
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
}

// TODO: 解决死锁问题
// 在 内核 中根据文件名打开一个根目录下的文件
pub fn open_file(name: &str, flags: OpenFlags) -> Option<Arc<OSInode>> {
    let (readable, writable) = flags.read_write();
    let name_string = String::from(name);
    let mut locked_map = HARD_LINK_MAP.lock();
    if flags.contains(OpenFlags::CREATE) {  
        if let Some(os_inode) = locked_map.get(&name_string) {
            // clear size
            // 如果文件已经存在则清空文件的内容
            let inner = os_inode.inner.lock();
            inner.inode.clear();
            Some(Arc::new(OSInode::new(
                readable,
                writable,
                Arc::clone(&inner.nlink),
                Arc::clone(&inner.inode),
            )))
        } else {
            // create file
            let inode = ROOT_INODE.create(name)
                .map(|inode| {
                    Arc::new(OSInode::new(
                        readable,
                        writable,
                        Arc::new(Mutex::new(LinkNumber(1 as usize))),
                        inode,
                    ))
                });
            locked_map.insert(name_string, inode.clone().unwrap());
            inode
        }
    } else {
        if let Some(os_inode) = locked_map.get(&name_string) {
            let inner = os_inode.inner.lock();
            if flags.contains(OpenFlags::TRUNC) {
                inner.inode.clear();
            }
            Some(Arc::new(OSInode::new(
                readable,
                writable,
                Arc::clone(&inner.nlink),
                Arc::clone(&inner.inode),
            )))
        } else {
            None
        }
    }
}

// 文件描述符层
impl File for OSInode {
    fn readable(&self) -> bool { self.readable }
    fn writable(&self) -> bool { self.writable }
    fn nlink(&self) -> usize { self.inner.lock().nlink.lock().0 }
    fn inode_id(&self) -> usize { self.inner.lock().inode.get_inode_id() }
    fn read(&self, mut buf: UserBuffer) -> usize {
        let mut inner = self.inner.lock();
        let mut total_read_size = 0usize;
        // 只需遍历 UserBuffer 中的每个缓冲区片段，调用 Inode 写好的 read/write_at 接口就好了
        for slice in buf.buffers.iter_mut() {
            let read_size = inner.inode.read_at(inner.offset, *slice);
            if read_size == 0 {
                break;
            }
            inner.offset += read_size; // offset 也随着遍历的进行被持续更新
            total_read_size += read_size;
        }
        total_read_size
    }
    fn write(&self, buf: UserBuffer) -> usize {
        let mut inner = self.inner.lock();
        let mut total_write_size = 0usize;
        for slice in buf.buffers.iter() {
            let write_size = inner.inode.write_at(inner.offset, *slice);
            assert_eq!(write_size, slice.len());
            inner.offset += write_size;
            total_write_size += write_size;
        }
        total_write_size
    }
}
