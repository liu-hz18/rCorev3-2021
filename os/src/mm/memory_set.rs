use super::{PageTable, PageTableEntry, PTEFlags};
use super::{VirtPageNum, VirtAddr, PhysPageNum, PhysAddr};
use super::{FrameTracker, frame_alloc};
use super::{VPNRange, StepByOne};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use riscv::register::satp;
use alloc::sync::Arc;
use lazy_static::*;
use spin::Mutex;
use crate::config::{
    MEMORY_END,
    PAGE_SIZE,
    TRAMPOLINE,
    TRAP_CONTEXT,
    USER_STACK_SIZE,
    MMIO
};

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

// 概括: 
// 启用分页模式下，内核代码的访存地址也会被视为一个虚拟地址并需要经过 MMU 的地址转换
// 因此我们也需要为内核对应构造一个 地址空间
// 它除了仍然需要允许内核的各数据段能够被正常访问之后，还需要包含所有应用的内核栈以及一个 跳板 (Trampoline)
// 跳板 放在最高的一个虚拟页面中
// 接下来则是从高到低放置每个应用的内核栈，内核栈的大小由 config 子模块的 KERNEL_STACK_SIZE 给出
// 它们的映射方式为 MapPermission 中的 rw 两个标志位，意味着这个逻辑段仅允许 CPU 处于内核态访问，且只能读或写
// 相邻两个内核栈之间会预留一个 保护页面 (Guard Page), 它是内核地址空间中的空洞，多级页表中并不存在与它相关的映射
// 它的意义在于当内核栈空间不足的时候，代码会尝试访问 空洞区域内的虚拟地址，然而它无法在多级页表中找到映射，便会触发异常，此时控制权会交给 trap handler 对这种情况进行 处理


// 创建内核地址空间的全局实例
lazy_static! {
    // 既需要 Arc<T> 提供的共享 引用，也需要 Mutex<T> 提供的互斥访问
    pub static ref KERNEL_SPACE: Arc<Mutex<MemorySet>> = Arc::new(Mutex::new(
        MemorySet::new_kernel()
    ));
}

pub fn kernel_token() -> usize {
    KERNEL_SPACE.lock().token()
}

// 地址空间：一系列有关联的逻辑段 (一般是指这些逻辑段属于一个运行的程序)
// 用来表明正在运行的应用所在执行环境中的可访问内存空间
// 在这个内存空间中，包含了一系列的不一定连续的逻辑段
// 当一个地址空间 MemorySet 生命周期结束后， 这些物理页帧都会被回收
pub struct MemorySet {
    page_table: PageTable, // PageTable 下 挂着所有多级页表的节点所在的物理页帧
    areas: Vec<MapArea>, // 对应逻辑段中的数据所在的物理页帧
}

impl MemorySet {
    pub fn new_bare() -> Self {
        // 新建一个空的地址空间
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    pub fn have_mapped(&self, vpn: &VirtPageNum) -> bool {
        for area in self.areas.iter() {
            if area.have_mapped(vpn) {
                return true;
            }
        }
        false
    }
    /// Assume that no conflicts.
    /// 在当前地址空间插入一个 Framed 方式映射到 物理内存的逻辑段
    /// 该方法的调用者要保证同一地址空间内的任意两个逻辑段不能存在交集
    pub fn insert_framed_area(&mut self, start_va: VirtAddr, end_va: VirtAddr, permission: MapPermission) {
        self.push(MapArea::new(
            start_va,
            end_va,
            MapType::Framed,
            permission,
        ), None);
    }
    // 只是将地址空间中的逻辑段列表 areas 清空，这将导致应用地址空间的所有数据被存放在的物理页帧被回收，而用来存放页表的那些物理页帧此时则不会被回收
    pub fn remove_area_with_start_vpn(&mut self, start_vpn: VirtPageNum) {
        if let Some((idx, area)) = self.areas.iter_mut().enumerate()
            .find(|(_, area)| area.vpn_range.get_start() == start_vpn) {
            area.unmap(&mut self.page_table);
            self.areas.remove(idx);
        }
    }
    // 在当前地址空间插入一个新的逻辑段 map_area
    // 如果它是以 Framed 方式映射到 物理内存，还可以可选地在那些被映射到的物理页帧上写入一些初始化数据 data
    pub fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }
    pub fn unmap(&mut self, vpn_range: VPNRange) {
        for vpn in vpn_range {
            for area in self.areas.iter_mut() { // this iterator yields `&` references
                if area.have_mapped(&vpn) {
                    area.unmap_one(&mut self.page_table, vpn);
                }
            }
        }
    }
    /// Mention that trampoline is not collected by areas.
    /// 注意无论是内核还是应用的地址空间，跳板页面均位于同样位置，且它们也将会映射到同一个实际存放这段 汇编代码的物理页帧。
    fn map_trampoline(&mut self) {
        // 并没有新增逻辑段 MemoryArea 而是直接在多级页表中插入一个从地址空间的最高虚拟页面映射到 跳板汇编代码所在的物理页帧的键值对，访问方式限制与代码段相同，即 RX 
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }
    /// Without kernel stacks.
    /// 创建内核地址空间
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        // 映射调班
        memory_set.map_trampoline();
        // map kernel sections
        println!("[kernel] .text [{:#x}, {:#x}) = {:#x} B", stext as usize, etext as usize, etext as usize-stext as usize);
        println!("[kernel] .rodata [{:#x}, {:#x}) = {:#x} B", srodata as usize, erodata as usize, erodata as usize - srodata as usize);
        println!("[kernel] .data [{:#x}, {:#x}) = {:#x} B", sdata as usize, edata as usize, edata as usize - sdata as usize);
        println!("[kernel] .bss [{:#x}, {:#x}) = {:#x} B", sbss_with_stack as usize, ebss as usize, ebss as usize - sbss_with_stack as usize);
        // 映射地址空间中最低 256GiB 中的所有的逻辑段
        // 从低地址到高地址 依次创建 5 个逻辑段并通过 push 方法将它们插入到内核地址空间中
        println!("[kernel] mapping .text section");
        memory_set.push(MapArea::new(
            (stext as usize).into(), // stext == BASE_ADDRESS
            (etext as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::X,
        ), None);
        println!("[kernel] mapping .rodata section");
        memory_set.push(MapArea::new(
            (srodata as usize).into(),
            (erodata as usize).into(),
            MapType::Identical,
            MapPermission::R,
        ), None);
        println!("[kernel] mapping .data section");
        memory_set.push(MapArea::new(
            (sdata as usize).into(),
            (edata as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::W,
        ), None);
        println!("[kernel] mapping .bss section");
        memory_set.push(MapArea::new(
            (sbss_with_stack as usize).into(),
            (ebss as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::W,
        ), None);
        println!("[kernel] mapping physical memory");
        memory_set.push(MapArea::new(
            (ekernel as usize).into(),
            MEMORY_END.into(),
            MapType::Identical,
            MapPermission::R | MapPermission::W,
        ), None);
        // 了能够在内核中访问 VirtIO 总线，我们就必须在内核地址空间中提前进行映射
        // 进行的是透明的恒等映射从而让内核可以兼容于直接访问物理地址的设备驱动库
        println!("[kernel] mapping memory-mapped registers");
        for pair in MMIO {
            memory_set.push(MapArea::new(
                (*pair).0.into(),
                ((*pair).0 + (*pair).1).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ), None);
        }
        memory_set
    }
    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp and entry point.
    /// 从应用的 ELF 格式可执行文件 解析出各数据段并对应生成应用的地址空间
    /// 对 get_app_data 得到的 ELF 格式数据进行解析
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        // map trampoline
        // 将跳板插入到应用地址空间
        memory_set.map_trampoline();
        // map program headers of elf, with U flag
        // 解析传入的应用 ELF 数据并可以轻松取出各个部分
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        // 取出 ELF 的魔数来判断 它是不是一个合法的 ELF
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0); // 记录目前涉及到的最大的虚拟页号
        // 我们可以直接得到 program header 的数目，然后遍历所有的 program header 并将合适的区域加入 到应用地址空间中
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            // Load: 它有被内核加载的必要，此时不必理会其他类型的 program header
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                // 计算这一区域在应用地址空间中的位置
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U; // 默认包含 U 标志
                // 确认这一区域访问方式的 限制并将其转换为 MapPermission 类型
                let ph_flags = ph.flags();
                if ph_flags.is_read() { map_perm |= MapPermission::R; }
                if ph_flags.is_write() { map_perm |= MapPermission::W; }
                if ph_flags.is_execute() { map_perm |= MapPermission::X; }
                // 创建逻辑段 map_area
                let map_area = MapArea::new(
                    start_va,
                    end_va,
                    MapType::Framed,
                    map_perm,
                );
                max_end_vpn = map_area.vpn_range.get_end();
                // push 到应用地址空间
                // 当前 program header 数据被存放的位置可以通过 ph.offset() 和 ph.file_size() 来找到
                // 注意: 当 存在一部分零初始化的时候， ph.file_size() 将会小于 ph.mem_size() ，
                // 因为这些零出于缩减可执行 文件大小的原因不应该实际出现在 ELF 数据中。
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize])
                );
            }
        }
        // 开始处理用户栈
        // map user stack with U flags
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into();
        // 紧接着在它上面再放置一个保护页面和用户栈即可
        // guard page
        user_stack_bottom += PAGE_SIZE;
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE;
        memory_set.push(MapArea::new(
            user_stack_bottom.into(),
            user_stack_top.into(),
            MapType::Framed,
            MapPermission::R | MapPermission::W | MapPermission::U,
        ), None);
        // map TrapContext, 映射次高页面来存放 Trap 上下文
        memory_set.push(MapArea::new(
            TRAP_CONTEXT.into(),
            TRAMPOLINE.into(),
            MapType::Framed,
            MapPermission::R | MapPermission::W,
        ), None);
        (
            memory_set, // 应用地址空间
            user_stack_top, // 用户栈虚拟地址 user_stack_top
            elf.header.pt2.entry_point() as usize // 从解析 ELF 得到的该应用入口点地址
        )
    }
    // 复制一个完全相同的地址空间
    pub fn from_existed_user(user_space: &MemorySet) -> MemorySet {
        // 新创建一个空的地址空间
        let mut memory_set = Self::new_bare();
        // map trampoline
        // 为这个地址空间映射上跳板页面
        memory_set.map_trampoline();
        // 剩下的逻辑段都包含在 areas 中
        // copy data sections/trap_context/user_stack
        for area in user_space.areas.iter() {
            let new_area = MapArea::from_another(area);
            // 在插入的时候就已经实际分配了物理页帧了
            memory_set.push(new_area, None);
            // copy data from another space
            // 遍历逻辑段中的每个虚拟页面，对应完成数据复制
            for vpn in area.vpn_range {
                // 找物理页帧
                let src_ppn = user_space.translate(vpn).unwrap().ppn();
                let dst_ppn = memory_set.translate(vpn).unwrap().ppn();
                dst_ppn.get_bytes_array().copy_from_slice(src_ppn.get_bytes_array());
            }
        }
        memory_set
    }
    pub fn activate(&self) {
        // 按照 satp CSR 格式要求 构造一个无符号 64 位无符号整数，使得其 分页模式为 SV39, 且将当前多级页表的根节点所在的物理页号填充进去
        // 从这一刻开始 SV39 分页模式就被启用了
        // 而且 MMU 会使用内核地址空间的多级页表进行地址转换
        let satp = self.page_table.token();
        // 一旦 我们修改了 satp 切换了地址空间，快表中的键值对就会失效，因为它还表示着上个地址空间的映射关系
        unsafe {
            satp::write(satp);
            llvm_asm!("sfence.vma" :::: "volatile");
        }
    }
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }
    pub fn recycle_data_pages(&mut self) {
        //*self = Self::new_bare();
        self.areas.clear();
    }
}

// 逻辑段
// 地址区间中的一段实际可用的地址连续的虚拟地址区间
// 该区间内包含的所有虚拟页面都以一种相同的方式映射到物理页帧，具有可读/可写/可执行等属性
pub struct MapArea {
    pub vpn_range: VPNRange, // 一段虚拟页号的连续区间, 是一个迭代器，可以使用 Rust 的语法糖 for-loop 进行迭代
    // 将这些物理页帧的生命周期绑定到它所在的逻辑段 MapArea 下
    data_frames: BTreeMap<VirtPageNum, FrameTracker>, // 保存了该逻辑段内的每个虚拟页面 和它被映射到的物理页帧 FrameTracker 的一个键值对容器 BTreeMap 中
    map_type: MapType, // 该逻辑段内的所有虚拟页面映射到物理页帧的同一种方式
    // 仅保留 U/R/W/X 四个标志位
    map_perm: MapPermission, // 控制该逻辑段的访问方式，它是页表项标志位 PTEFlags 的一个子集
}

impl MapArea {
    // 新建一个逻辑段结构体，
    // 注意传入的起始/终止虚拟地址会分别被下取整/上取整为虚拟页号 并传入 迭代器 vpn_range 中
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        // println!("map: s_va={:?}, e_va={:?}, s_vpn={:?}, e_vpn={:?}", start_va, end_va, start_vpn, end_vpn);
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
    // 从一个逻辑段 复制得到一个 虚拟地址区间、映射方式和权限控制均相同 的逻辑段
    // 不同的是由于它还没有真正被映射到物理页帧上，所以 data_frames 字段为空
    pub fn from_another(another: &MapArea) -> Self {
        Self {
            vpn_range: VPNRange::new(another.vpn_range.get_start(), another.vpn_range.get_end()),
            data_frames: BTreeMap::new(),
            map_type: another.map_type,
            map_perm: another.map_perm,
        }
    }
    // 单个虚拟页面进行映射/解映射
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        // 虚拟页号 vpn 已经确定
        let ppn: PhysPageNum;
        // 页表项的 物理页号则取决于当前逻辑段映射到物理内存的方式
        match self.map_type {
            // 恒等映射 Identical 方式映射的时候，物理页号就等于虚拟页号
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            // Framed 方式映射的时候，需要分配一个物理页帧让当前的虚拟页面可以映射过去
            // 此时页表项中的物理页号自然就是 这个被分配的物理页帧的物理页号
            // 还需要将这个物理页帧挂在逻辑段的 data_frames 字段下
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }
        // 页表项的标志位来源于当前逻辑段的类型为 MapPermission 的统一配置
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
    }
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        match self.map_type {
            // 当以 Framed 映射的时候，不要忘记同时将虚拟页面被映射到的物理页帧 FrameTracker 从 data_frames 中移除
            // 这样这个物理页帧才能立即被回收以备后续分配
            MapType::Framed => {
                self.data_frames.remove(&vpn);
            }
            _ => {}
        }
        page_table.unmap(vpn); // 删除以传入的虚拟页号为键的 键值对即可
    }
    // 将 当前逻辑段到物理内存的映射 从传入的该逻辑段所属的地址空间的多级页表page_table中 加入或删除
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }
    pub fn have_mapped(&self, vpn: &VirtPageNum) -> bool {
        self.data_frames.contains_key(vpn)
    }
    // 将切片 data 中的数据 拷贝到 当前逻辑段实际被内核放置在的各物理页帧 上
    // 切片 data 中的数据大小不超过当前逻辑段的 总大小
    // 切片中的数据会被对齐到逻辑段的开头，然后逐页拷贝到实际的物理页帧
    /// data: start-aligned but maybe with shorter length
    /// assume that all frames were cleared before
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        // 遍历每一个需要拷贝数据的虚拟页面
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            // 从传入的当前逻辑段所属的地址空间的多级页表中手动查找迭代到的虚拟页号被映射 到的物理页帧
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()]; // 能够真正改写该物理页帧上内容的字节数组型可变引用
            // 直接使用 copy_from_slice 完成复制
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }
}

// 该逻辑段内的所有虚拟页面映射到物理页帧的同一种方式
// 
#[derive(Copy, Clone, PartialEq, Debug)]
pub enum MapType {
    Identical, // 恒等映射, 用于在启用多级页表之后仍能够访问一个特定的物理地址指向的物理内存
    Framed, // 每个虚拟页面都需要映射到一个新分配的物理页帧
}

// 仅保留 U/R/W/X 四个标志位，因为其他的标志位仅与硬件的地址转换机制细节相关
bitflags! {
    pub struct MapPermission: u8 {
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
    }
}

// 检查内核地址空间的多级页表是否被正确设置
#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.lock();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    // 分别通过手动查内核多级页表的方式验证 代码段 和 只读数据段 不允许被写入，同时不允许从 数据段 上取指
    assert_eq!(
        kernel_space.page_table.translate(mid_text.floor()).unwrap().writable(),
        false
    );
    assert_eq!(
        kernel_space.page_table.translate(mid_rodata.floor()).unwrap().writable(),
        false,
    );
    assert_eq!(
        kernel_space.page_table.translate(mid_data.floor()).unwrap().executable(),
        false,
    );
    println!("[kernel] remap_test passed!");
}
