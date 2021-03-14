use crate::config::{PAGE_SIZE, PAGE_SIZE_BITS};
use super::PageTableEntry;
use core::fmt::{self, Debug, Formatter};

// Rust 的元组式结构体，可以看成 usize 的一种简单包装
// 为了在 Rust 编译器的帮助下进行 多种方便且安全的 类型转换 (Type Convertion) 

/// Definitions
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);

/// Debugging

impl Debug for VirtAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl Debug for VirtPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#x}", self.0))
    }
}
impl Debug for PhysAddr {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl Debug for PhysPageNum {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#x}", self.0))
    }
}

/// T: {PhysAddr, VirtAddr, PhysPageNum, VirtPageNum}
/// T -> usize: T.0
/// usize -> T: usize.into()
// 实现 T 和 usize 之间的类型转换
// PhysAddr::from(_: usize)
// pa.0
impl From<usize> for PhysAddr {
    fn from(v: usize) -> Self { Self(v) }
}
impl From<usize> for PhysPageNum {
    fn from(v: usize) -> Self { Self(v) }
}
impl From<usize> for VirtAddr {
    fn from(v: usize) -> Self { Self(v) }
}
impl From<usize> for VirtPageNum {
    fn from(v: usize) -> Self { Self(v) }
}
impl From<PhysAddr> for usize {
    fn from(v: PhysAddr) -> Self { v.0 }
}
impl From<PhysPageNum> for usize {
    fn from(v: PhysPageNum) -> Self { v.0 }
}
impl From<VirtAddr> for usize {
    fn from(v: VirtAddr) -> Self { v.0 }
}
impl From<VirtPageNum> for usize {
    fn from(v: VirtPageNum) -> Self { v.0 }
}

// 地址和页号之间可以相互转换
impl VirtAddr {
    pub fn floor(&self) -> VirtPageNum { VirtPageNum(self.0 / PAGE_SIZE) }
    pub fn ceil(&self) -> VirtPageNum  { VirtPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE) }
    pub fn page_offset(&self) -> usize { self.0 & (PAGE_SIZE - 1) }
    pub fn aligned(&self) -> bool { self.page_offset() == 0 }
}
// 虚拟地址 转 虚拟页号
// 地址需要 保证它与页面大小对齐 才能通过右移转换为 页号
// 对于不对齐的情况，物理地址不能通过 From/Into 转换为物理页号
// 需要通过它自己的 floor 或 ceil 方法来 进行 下取整 或 上取整 的转换
impl From<VirtAddr> for VirtPageNum {
    fn from(v: VirtAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}
// 虚拟页号 转 虚拟地址，低12位补0
impl From<VirtPageNum> for VirtAddr {
    fn from(v: VirtPageNum) -> Self { Self(v.0 << PAGE_SIZE_BITS) }
}
impl PhysAddr {
    pub fn floor(&self) -> PhysPageNum { PhysPageNum(self.0 / PAGE_SIZE) }
    pub fn ceil(&self) -> PhysPageNum { PhysPageNum((self.0 - 1 + PAGE_SIZE) / PAGE_SIZE) }
    pub fn page_offset(&self) -> usize { self.0 & (PAGE_SIZE - 1) }
    pub fn aligned(&self) -> bool { self.page_offset() == 0 }
}
impl From<PhysAddr> for PhysPageNum {
    fn from(v: PhysAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.floor()
    }
}
impl From<PhysPageNum> for PhysAddr {
    fn from(v: PhysPageNum) -> Self { Self(v.0 << PAGE_SIZE_BITS) }
}

// 虚拟页号的 三级页索引，并按照 从高到低 的顺序返回
impl VirtPageNum {
    pub fn indexes(&self) -> [usize; 3] {
        let mut vpn = self.0;
        let mut idx = [0usize; 3];
        for i in (0..3).rev() {
            idx[i] = vpn & 511; // 只取出低 27 位
            vpn >>= 9;
        }
        idx
    }
}

impl PhysAddr {
    pub fn get_mut<T>(&self) -> &'static mut T {
        unsafe {
            (self.0 as *mut T).as_mut().unwrap()
        }
    }
    pub fn get_ref<T>(&self) -> &'static T {
        unsafe {
            (self.0 as *const T).as_ref().unwrap()
        }
    }
}

// S, U态 都是虚拟地址了
// 内核中访问物理页帧: 为了简单，使用 恒等映射，虚拟页号就是对应的物理页号(仅在内核地址空间中)
impl PhysPageNum {
    // 返回：一个 页表项定长数组 的可变引用，可以用来修改多级页表中的一个节点
    pub fn get_pte_array(&self) -> &'static mut [PageTableEntry] {
        // 先把物理页号转为物理地址 PhysAddr
        let pa: PhysAddr = self.clone().into();
        // 直接将它 转为裸指针用来访问物理地址指向的物理内存
        // 虽然裸指针被视为一个虚拟地址， 但是已经提到在内核空间中虚拟地址会映射到一个相同的物理地址，因此在这种情况下也成立
        unsafe {
            core::slice::from_raw_parts_mut(pa.0 as *mut PageTableEntry, 512)
        }
    }
    // 返回：一个 字节数组 的可变引用，可以以字节为粒度对物理页帧上的数据进行访问
    pub fn get_bytes_array(&self) -> &'static mut [u8] {
        let pa: PhysAddr = self.clone().into();
        unsafe {
            core::slice::from_raw_parts_mut(pa.0 as *mut u8, 4096)
        }
    }
    // 获取一个恰好放在 一个物理页帧开头 的类型为 T 的数据的可变引用
    pub fn get_mut<T>(&self) -> &'static mut T {
        let pa: PhysAddr = self.clone().into();
        pa.get_mut()
    }
}

pub trait StepByOne {
    fn step(&mut self);
}
impl StepByOne for VirtPageNum {
    fn step(&mut self) {
        self.0 += 1;
    }
}
impl StepByOne for PhysPageNum {
    fn step(&mut self) {
        self.0 += 1;
    }
}

#[derive(Copy, Clone)]
pub struct SimpleRange<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
    l: T,
    r: T,
}
impl<T> SimpleRange<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
    pub fn new(start: T, end: T) -> Self {
        assert!(start <= end, "start {:?} > end {:?}!", start, end);
        Self { l: start, r: end }
    }
    pub fn get_start(&self) -> T { self.l }
    pub fn get_end(&self) -> T { self.r }
}
impl<T> IntoIterator for SimpleRange<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
    type Item = T;
    type IntoIter = SimpleRangeIterator<T>;
    fn into_iter(self) -> Self::IntoIter {
        SimpleRangeIterator::new(self.l, self.r)
    }
}
pub struct SimpleRangeIterator<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
    current: T,
    end: T,
}
impl<T> SimpleRangeIterator<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
    pub fn new(l: T, r: T) -> Self {
        Self { current: l, end: r, }
    }
}
impl<T> Iterator for SimpleRangeIterator<T> where
    T: StepByOne + Copy + PartialEq + PartialOrd + Debug, {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current == self.end {
            None
        } else {
            let t = self.current;
            self.current.step();
            Some(t)
        }
    }
}
pub type VPNRange = SimpleRange<VirtPageNum>;
