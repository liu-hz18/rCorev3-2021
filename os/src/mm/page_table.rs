use super::{frame_alloc, PhysPageNum, FrameTracker, VirtPageNum, VirtAddr, StepByOne};
use alloc::vec::Vec;
use alloc::vec;
use alloc::string::String;
use bitflags::*;
use crate::mm::{PhysAddr};

// 在我们切换任务的时候， satp 也必须被同时切换
bitflags! {
    // 将一个 u8 封装成一个标志位的集合类型
    pub struct PTEFlags: u8 {
        const V = 1 << 0; // 仅当 V(Valid) 位为 1 时，页表项才是合法的
        const R = 1 << 1; // R/W/X 分别控制索引到这个页表项的对应虚拟页面是否允许 读/写/取指
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4; // 控制索引到这个页表项的对应虚拟页面是否在 CPU 处于 U 特权级的情况下是否被允许访问
        const G = 1 << 5;
        const A = 1 << 6; // 记录自从页表项上的这一位被清零之后，页表项的对应 虚拟页面 是否被 访问 过
        const D = 1 << 7; // 记录自从页表项上的这一位被清零之后，页表项的对应 虚拟页面 是否被 修改 过
    }
    // 当 V 为 1 且 R/W/X 均为 0 时，表示是一个合法的页目录表项，其包含的指针会指向下一级的页表
    // 当 V 为 1 且 R/W/X 不全为 0 时，表示是一个合法的页表项，其包含了虚地址对应的物理页号
    // 只要 R/W/X 不全为 0 就会停下来，直接从当前的页表项中取出物理页号进行最终的地址转换
}

// 页表项 (PTE, Page Table Entry) 
// Copy, Clone: 以值语义赋值/传参的时候 不会发生所有权转移，而是拷贝一份新的副本
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PageTableEntry {
    pub bits: usize,
}

impl PageTableEntry {
    // 从一个物理页号 PhysPageNum 和一个页表项标志位 PTEFlags 生成一个页表项 PageTableEntry 实例
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    // 生成一个全零的页表项, 隐含着该页表项的 V 标志位为 0，因此它是不合法的 
    pub fn empty() -> Self {
        PageTableEntry {
            bits: 0,
        }
    }
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    pub fn is_valid(&self) -> bool {
        // &: PTEFlags实现的逻辑运算，相当于判断两个集合的交集是否为空集
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

// 每个应用的地址空间都对应一个不同的多级页表，这也就意味这不同页表的起始地址（即页表根节点的地址）是不一样的
// PageTable 要保存它根节点的物理页号 root_ppn 作为页表唯一的区分标志
// NOTE: 当 PageTable 生命周期结束后，向量 frames 里面的那些 FrameTracker 也会被回收，也就意味着存放多级页表节点的那些物理页帧 被回收了
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>, // 保存了页表所有的节点（包括根节点）所在的物理页帧
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    pub fn new() -> Self {
        // 分配一个物理页帧 FrameTracker 并挂在向量 frames 下
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn, // 更新根节点的物理页号 root_ppn
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    // 临时创建一个专用来手动查页表的 PageTable
    pub fn from_token(satp: usize) -> Self {
        // 仅有一个从传入的 satp token 中得到的多级页表根节点 的 物理页号
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(), // frames 字段为空，也即不实际控制任何资源
        }
    }
    // 从vpn找ppn, 找不到的时候就创建
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn; // 当前节点的物理页号, 最开始指向多级页表的根节点
        let mut result: Option<&mut PageTableEntry> = None;
        // 通过 get_pte_array 将 取出当前节点的 页表项数组
        for i in 0..3 {
            let pte = &mut ppn.get_pte_array()[idxs[i]]; // 并根据当前级页索引找到对应的页表项
            if i == 2 { // 如果当前节点是一个叶节点，那么直接返回这个页表项 的可变引用
                result = Some(pte);
                break;
            }
            // 如果在 遍历的过程中发现有节点尚未创建则会新建一个节点
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V); // 更新作为下级节点指针的页表项
                self.frames.push(frame); // 将新分配的物理页帧移动到 向量 frames 中方便后续的自动回收
            }
            ppn = pte.ppn();
        }
        result
    }
    // 从vpn找ppn, 找不到的时候就返回None
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&PageTableEntry> = None;
        for i in 0..3 {
            let pte = &ppn.get_pte_array()[idxs[i]];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    // 在多级页表中插入一个 <虚拟页号，物理页号> 键值对，
    // 注意这里我们将物理页号 ppn 和页表项标志位 flags 作为 不同的参数传入而不是整合为一个页表项
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        // 只需根据虚拟页号找到页表项
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        // 修改其内容
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    // 删除一个 <虚拟页号，物理页号> 键值对
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte_create(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        // 直接清空页表项内容
        *pte = PageTableEntry::empty();
    }
    // 如果能够找到页表项，那么它会将页表项拷贝一份并返回
    // 否则就 返回一个 None
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn)
            .map(|pte| {pte.clone()})
    }
    pub fn translate_va(&self, va: VirtAddr) -> Option<PhysAddr> {
        self.find_pte(va.clone().floor())
            .map(|pte| {
                //println!("translate_va:va = {:?}", va);
                let aligned_pa: PhysAddr = pte.ppn().into();
                //println!("translate_va:pa_align = {:?}", aligned_pa);
                let offset = va.page_offset();
                let aligned_pa_usize: usize = aligned_pa.into();
                (aligned_pa_usize + offset).into()
            })
    }
    pub fn translate_pte(&self, va: VirtAddr) -> Option<PageTableEntry> {
        let vpn = va.floor();
        self.translate(vpn)
    }
    // satp token
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
}

// 将 应用地址空间中一个缓冲区 转化为在 内核空间中能够直接访问 的形式
pub fn translated_byte_buffer(
    token: usize, // 某个应用地址空间的 token 
    ptr: *const u8, // 该应用 虚拟地址空间中 的一段缓冲区的起始地址 和长度
    len: usize
) -> Vec<&'static mut [u8]> { // 以 向量 的形式返回一组可以在内核空间中直接访问的 字节数组切片
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table
            .translate(vpn)
            .unwrap()
            .ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        start = end_va.into();
    }
    v
}

pub fn virtual_addr_printable(token: usize, va: usize) -> (bool, usize) {
    let va = VirtAddr::from(va);
    let page_table = PageTable::from_token(token);
    if let Some(pte) = page_table.translate_pte(va) {
        (
            pte.is_valid() && (pte.readable() && !pte.executable()),
            page_table.translate_va(va).unwrap().0
        )
    } else {
        (false, 0)
    }
}

pub fn virtual_addr_writable(token: usize, va: usize) -> bool {
    let va = VirtAddr::from(va);
    let page_table = PageTable::from_token(token);
    if let Some(pte) = page_table.translate_pte(va) {
        pte.is_valid() && pte.readable() && pte.writable()
    } else {
        false
    }
}

pub fn virtual_addr_range_printable(token: usize, ptr: *const u8, len: usize) -> (bool, usize, usize) {
    let (start_printable, start_pa) = virtual_addr_printable(token, ptr as usize);
    let (end_printable, end_pa) = virtual_addr_printable(token, ptr as usize + len);
    (
        start_printable && end_printable,
        start_pa,
        end_pa
    )
}

pub fn virtual_addr_range_writable(token: usize, ptr: *const u8, len: usize) -> bool {
    let start_writable = virtual_addr_writable(token, ptr as usize);
    let end_writable = virtual_addr_writable(token, ptr as usize + len);
    start_writable && end_writable
}

pub fn translated_virtual_ptr<T>(
    token: usize,
    v_ptr: *mut T,
) -> *mut T {
    let page_table = PageTable::from_token(token);
    let pa: usize = page_table.translate_va(VirtAddr::from(v_ptr as usize)).unwrap().0;
    pa as *mut T
}

// 从内核地址空间之外的某个地址空间中拿到一个字符串，其原理就是逐字节查页表直到发现一个 \0 为止
pub fn translated_str(token: usize, ptr: *const u8) -> String {
    let page_table = PageTable::from_token(token);
    let mut string = String::new();
    let mut va = ptr as usize;
    loop {
        let ch: u8 = *(page_table.translate_va(VirtAddr::from(va)).unwrap().get_mut());
        if ch == 0 {
            break;
        } else {
            string.push(ch as char);
            va += 1;
        }
    }
    string
}

pub fn translated_refmut<T>(token: usize, ptr: *mut T) -> &'static mut T {
    //println!("into translated_refmut!");
    let page_table = PageTable::from_token(token);
    let va = ptr as usize;
    //println!("translated_refmut: before translate_va");
    page_table.translate_va(VirtAddr::from(va)).unwrap().get_mut()
}

// 应用虚拟地址空间中的一段缓冲区的抽象, 存放的是一些 虚拟地址区间
// 本质上其实只是一个 &[u8], 给出了缓冲区的起始地址及长度
// 但是它位于应用地址空间中，在内核中我们无法直接通过这种方式来访问，因此需要进行封装
pub struct UserBuffer {
    pub buffers: Vec<&'static mut [u8]>,
}

impl UserBuffer {
    pub fn new(buffers: Vec<&'static mut [u8]>) -> Self {
        Self { buffers }
    }
    pub fn len(&self) -> usize {
        let mut total: usize = 0;
        for b in self.buffers.iter() {
            total += b.len();
        }
        total
    }
}

impl IntoIterator for UserBuffer {
    type Item = *mut u8;
    type IntoIter = UserBufferIterator;
    fn into_iter(self) -> Self::IntoIter {
        UserBufferIterator {
            buffers: self.buffers,
            current_buffer: 0,
            current_idx: 0,
        }
    }
}

pub struct UserBufferIterator {
    buffers: Vec<&'static mut [u8]>,
    current_buffer: usize,
    current_idx: usize,
}

// 作为一个迭代器可以逐字节进行读写
impl Iterator for UserBufferIterator {
    type Item = *mut u8;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_buffer >= self.buffers.len() {
            None
        } else {
            let r = &mut self.buffers[self.current_buffer][self.current_idx] as *mut _;
            if self.current_idx + 1 == self.buffers[self.current_buffer].len() {
                self.current_idx = 0;
                self.current_buffer += 1;
            } else {
                self.current_idx += 1;
            }
            Some(r)
        }
    }
}
