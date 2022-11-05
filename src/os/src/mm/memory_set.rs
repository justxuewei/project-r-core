use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use bitflags::*;
use core::arch::asm;
use lazy_static::*;

use crate::{
    config::{self, MEMORY_END, PAGE_SIZE, TRAMPOLINE},
    mm::address::StepByOne,
    sync::UPSafeCell,
};

use super::{
    address::{PhysAddr, PhysPageNum, VPNRange, VirtAddr, VirtPageNum},
    frame_allocator::{frame_alloc, FrameTracker},
    page_table::{PTEFlags, PageTable, PageTableEntry},
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

lazy_static! {
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> =
        Arc::new(unsafe { UPSafeCell::new(MemorySet::new_kernel()) });
}

// Get kernel space root ppn
pub fn kernel_token() -> usize {
    KERNEL_SPACE.exclusive_access().token()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MapType {
    Identical, // 一个 VPN 唯一的映射一个 PPN，比如内核就需要访问物理内存中的某个 PPN
    Framed,    // 一个 VPN 随机的映射一个 PPN
}

bitflags! {
    // a subset of PTEFlags
    pub struct MapPermission: u8 {
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
    }
}

#[derive(Debug)]
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

impl MapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        Self {
            vpn_range: VPNRange::new(start_va.floor(), end_va.ceil()),
            data_frames: BTreeMap::new(),
            map_type: map_type,
            map_perm: map_perm,
        }
    }

    // 拷贝一个与 `map_area` 一样长度和位置的虚拟地址空间，
    // 但是不拷贝页框数据。
    pub fn from_another(map_area: &MapArea) -> Self {
        Self {
            vpn_range: VPNRange::new(map_area.vpn_range.get_start(), map_area.vpn_range.get_end()),
            data_frames: BTreeMap::new(),
            map_type: map_area.map_type,
            map_perm: map_area.map_perm,
        }
    }

    // map_one 为一个 vpn 申请一个物理页框，
    // 将 vpn 和 ppn 的映射关系保存到 page table 中。
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        let ppn: PhysPageNum;
        match self.map_type {
            MapType::Identical => ppn = PhysPageNum(vpn.0),
            MapType::Framed => {
                let frame = frame_alloc().unwrap();
                ppn = frame.ppn;
                self.data_frames.insert(vpn, frame);
            }
        }

        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
    }

    #[allow(unused)]
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        if self.map_type == MapType::Framed {
            self.data_frames.remove(&vpn);
        }
        page_table.unmap(vpn);
    }

    // map 将逻辑段包含的所有 vpn 与 ppn 的映射关系保存到 page table 中
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn);
        }
    }

    #[allow(unused)]
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn)
        }
    }

    // copy_data 将 data 的数据拷贝到当前逻辑段中对应的物理内存中。
    // 需要注意的是 data 长度不能超过当前逻辑段的长度，按页为单位拷贝。
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }
}

#[derive(Debug)]
pub struct MemorySet {
    page_table: PageTable,
    areas: Vec<MapArea>,
}

impl MemorySet {
    fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: Vec::new(),
        }
    }

    pub fn token(&self) -> usize {
        self.page_table.token()
    }

    // insert_framed_area 将逻辑地址映射到 memory set 中。
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }

    // 从 memory_set 中移除一个指定的 map_area
    pub fn remove_area_with_start_vpn(&mut self, start_vpn: VirtPageNum) {
        if let Some((idx, area)) = self
            .areas
            .iter_mut()
            .enumerate()
            .find(|(_, area)| area.vpn_range.get_start() == start_vpn)
        {
            area.unmap(&mut self.page_table);
            self.areas.remove(idx);
        }
    }

    // push 将逻辑段内容映射到物理内存中，如果有数据则深拷贝数据，最后将 map_area 保存到 mmset 中。
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        map_area.map(&mut self.page_table);
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        self.areas.push(map_area);
    }

    fn map_trampoline(&mut self) {
        let vpn: VirtPageNum = VirtAddr::from(TRAMPOLINE).into();
        let ppn: PhysPageNum = PhysAddr::from(strampoline as usize).into();
        self.page_table.map(vpn, ppn, PTEFlags::R | PTEFlags::X);
    }

    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // high kernel address space
        memory_set.map_trampoline();

        // low kernel address space
        println!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        println!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        println!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        println!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );

        println!("mapping .text section");
        let text_map_area = MapArea::new(
            (stext as usize).into(),
            (etext as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::X,
        );
        memory_set.push(text_map_area, None);

        println!("mapping .rodata section");
        let rodata_map_area = MapArea::new(
            (srodata as usize).into(),
            (erodata as usize).into(),
            MapType::Identical,
            MapPermission::R,
        );
        memory_set.push(rodata_map_area, None);

        println!("mapping .data section");
        let data_map_area = MapArea::new(
            (sdata as usize).into(),
            (edata as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::W,
        );
        memory_set.push(data_map_area, None);

        println!("mapping .bss section");
        let bss_map_area = MapArea::new(
            (sbss_with_stack as usize).into(),
            (ebss as usize).into(),
            MapType::Identical,
            MapPermission::R | MapPermission::W,
        );
        memory_set.push(bss_map_area, None);

        println!("mapping physical memory");
        let phy_mem_map_area = MapArea::new(
            (ekernel as usize).into(),
            MEMORY_END.into(),
            MapType::Identical,
            MapPermission::R | MapPermission::W,
        );
        memory_set.push(phy_mem_map_area, None);

        println!("mapping memory-mapped registers");
        for (started_address, length) in config::MMIO {
            let start_va: VirtAddr = (*started_address).into();
            let end_va: VirtAddr = (*started_address + *length).into();
            memory_set.push(
                MapArea::new(
                    start_va,
                    end_va,
                    MapType::Identical,
                    MapPermission::R | MapPermission::W,
                ),
                None,
            );
        }

        println!("kernel's memory set was loaded");

        memory_set
    }

    // from_elf 根据 elf 文件创建一个 mmset，
    // 完成的事情包括验证 elf 文件是否合法，根据 program headers 加载数据的逻辑段，
    // 设置 user stack，以及设置 trap context 地址。
    //
    //     low addr
    //   ┌───────────┐ ◀────── (bottom)
    //   │           │
    //   │ UserStack │
    //   │           │
    //   └───────────┘ ◀────── ustack_base (top)
    //     high addr
    //
    // returns:
    //  - memory_set
    //  - user stack 栈顶虚拟地址
    //  - app 入口地址
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        memory_set.map_trampoline();

        // read elf header
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");

        // load program from program headers
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }

        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut ustack_base: usize = max_end_va.into();
        // guard page
        ustack_base += config::PAGE_SIZE;

        (
            memory_set,
            ustack_base,
            elf.header.pt2.entry_point() as usize,
        )
    }

    //  创建并拷贝一个已有用户地址空间 (memory_set)
    pub fn from_existed_user(user_space: &MemorySet) -> Self {
        let mut memory_set = Self::new_bare();
        memory_set.map_trampoline();

        for area in user_space.areas.iter() {
            let new_map_area = MapArea::from_another(area);
            memory_set.push(new_map_area, None);
            for vpn in area.vpn_range {
                let src_ppn = user_space.translate(vpn).unwrap().ppn();
                let dst_ppn = memory_set.translate(vpn).unwrap().ppn();
                dst_ppn
                    .get_bytes_array()
                    .copy_from_slice(src_ppn.get_bytes_array());
            }
        }

        memory_set
    }

    // activate 设置根页表地址并启用 SV39 分页
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            riscv::register::satp::write(satp);
            // 写屏障
            asm!("sfence.vma");
        }
    }

    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }

    /// 移除全部的内存，比如代码区、bss 区等
    pub fn recycle_data_pages(&mut self) {
        self.areas.clear()
    }
}
