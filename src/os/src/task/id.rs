use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};

use crate::{
    config::{KERNEL_STACK_SIZE, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE},
    mm::{
        address::{PhysPageNum, VirtAddr},
        memory_set::MapPermission,
        KERNEL_SPACE,
    },
    sync::UPSafeCell,
};

use lazy_static::*;

use super::process::ProcessControlBlock;

/// TaskUserRes 保存线程执行的必要信息，在线程初始化之后就不会再变化了。
pub struct TaskUserRes {
    // 线程 id
    pub tid: usize,
    // 用户栈顶地址
    pub ustack_base: usize,
    pub process: Weak<ProcessControlBlock>,
}

impl TaskUserRes {
    /// 创建用户资源，包括 tid（线程 id）、线程用户栈以及 TrapContext。
    pub fn new(
        process: Arc<ProcessControlBlock>,
        ustack_base: usize,
        alloc_user_res: bool,
    ) -> Self {
        let tid = process.inner_exclusive_access().alloc_tid();
        let task_user_res = TaskUserRes {
            tid,
            ustack_base,
            process: Arc::downgrade(&process),
        };
        if alloc_user_res {
            task_user_res.alloc_user_res();
        }
        task_user_res
    }

    /// 为当前线程申请并初始化一个线程用户栈和 TrapContext。
    pub fn alloc_user_res(&self) {
        let process = self.process.upgrade().unwrap();
        let mut inner = process.inner_exclusive_access();
        // 初始化当前线程的 ustack
        let ustack_bottom = ustack_bottom_from_tid(self.ustack_base, self.tid);
        let ustack_top = ustack_bottom + USER_STACK_SIZE;
        inner.memory_set.insert_framed_area(
            ustack_bottom.into(),
            ustack_top.into(),
            MapPermission::R | MapPermission::W | MapPermission::U,
        );
        // 初始化 trap context
        let trap_cx_bottom = trap_cx_bottom_from_tid(self.tid);
        let trap_cx_top = trap_cx_bottom + PAGE_SIZE;
        inner.memory_set.insert_framed_area(
            trap_cx_bottom.into(),
            trap_cx_top.into(),
            MapPermission::R | MapPermission::W,
        );
    }

    pub fn alloc_tid(&mut self) -> usize {
        self.process
            .upgrade()
            .unwrap()
            .inner_exclusive_access()
            .alloc_tid()
    }

    pub fn dealloc_tid(&mut self, tid: usize) {
        self.process
            .upgrade()
            .unwrap()
            .inner_exclusive_access()
            .dealloc_tid(tid)
    }

    pub fn ustack_top(&self) -> usize {
        ustack_bottom_from_tid(self.ustack_base, self.tid) + USER_STACK_SIZE
    }

    fn dealloc_user_res(&self) {
        let process = self.process.upgrade().unwrap();
        let mut inner = process.inner_exclusive_access();
        // dealloc ustack
        inner
            .memory_set
            .remove_area_with_start_vpn(ustack_bottom_from_tid(self.ustack_base, self.tid).into());
        // dealloc trap context
        inner
            .memory_set
            .remove_area_with_start_vpn(trap_cx_bottom_from_tid(self.tid).into());
    }

    /// 获取 TrapContext 的物理页号（PhysPageNum）
    pub fn trap_cx_ppn(&self) -> PhysPageNum {
        let process = self.process.upgrade().unwrap();
        let process_inner = process.inner_exclusive_access();
        let trap_cx_va: VirtAddr = trap_cx_bottom_from_tid(self.tid).into();
        process_inner
            .memory_set
            .translate(trap_cx_va.into())
            .unwrap()
            .ppn()
    }
}

/// 获取 user stack 的底端
fn ustack_bottom_from_tid(ustack_base: usize, tid: usize) -> usize {
    ustack_base + tid * (USER_STACK_SIZE + PAGE_SIZE)
}

/// 获取 trap context 的底端（TRAMPOLINE）
fn trap_cx_bottom_from_tid(tid: usize) -> usize {
    TRAP_CONTEXT_BASE - tid * PAGE_SIZE
}

pub struct RecycleAllocator {
    current: usize,
    recycled: Vec<usize>,
}

impl RecycleAllocator {
    pub fn new() -> Self {
        RecycleAllocator {
            current: 0,
            recycled: Vec::new(),
        }
    }

    pub fn alloc(&mut self) -> usize {
        if let Some(id) = self.recycled.pop() {
            id
        } else {
            self.current += 1;
            self.current - 1
        }
    }

    pub fn dealloc(&mut self, id: usize) {
        assert!(id < self.current);
        assert!(
            self.recycled.iter().any(|i| *i == id),
            "id {} has been deallocated",
            id
        );
        self.recycled.push(id);
    }
}

impl Drop for TaskUserRes {
    fn drop(&mut self) {
        self.dealloc_tid(self.tid);
        self.dealloc_user_res();
    }
}

lazy_static! {
    static ref PID_ALLOCATOR: UPSafeCell<RecycleAllocator> =
        unsafe { UPSafeCell::new(RecycleAllocator::new()) };
    static ref KSTACK_ALLOCATOR: UPSafeCell<RecycleAllocator> =
        unsafe { UPSafeCell::new(RecycleAllocator::new()) };
}

pub const IDLE_PID: usize = 0;

pub struct PidHandle(pub usize);

pub fn pid_alloc() -> PidHandle {
    PidHandle(PID_ALLOCATOR.exclusive_access().alloc())
}

impl Drop for PidHandle {
    fn drop(&mut self) {
        PID_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

pub struct KernelStack(pub usize);

impl Drop for KernelStack {
    fn drop(&mut self) {
        let (bottom, _) = kernel_stack_position(self.0);
        let bottom_va = VirtAddr::from(bottom);
        KERNEL_SPACE
            .exclusive_access()
            .remove_area_with_start_vpn(bottom_va.into());
        KSTACK_ALLOCATOR.exclusive_access().dealloc(self.0);
    }
}

impl KernelStack {
    /// 将 T 压入内核栈的顶端，如下所示：
    ///                                                     
    /// ┌───────────┐◀───bottom                ┌───────────┐
    /// │           │                          │           │
    /// │  kstack   │        ━━push_on_top━━▶  ├───────────┤
    /// │           │                          │     T     │
    /// └───────────┘◀───top                   └───────────┘
    ///                                                     
    ///   high addr                              high addr  
    pub fn push_on_top<T: Sized>(&self, value: T) -> *mut T {
        let top = self.get_top();
        let ptr = (top - core::mem::size_of::<T>()) as *mut T;
        unsafe {
            *ptr = value;
        }
        ptr
    }

    /// 获取内核栈（kstack）的顶部，使用 kernel_stack_position 第二个返回值，有
    /// 关内存布局详见 kernel_stack_position 的注释。
    pub fn get_top(&self) -> usize {
        let (_, top) = kernel_stack_position(self.0);
        top
    }
}

/// 申请一个内核栈（kstack）
pub fn kstack_alloc() -> KernelStack {
    let kstack_id = KSTACK_ALLOCATOR.exclusive_access().alloc();
    let (bottom, top) = kernel_stack_position(kstack_id);
    KERNEL_SPACE.exclusive_access().insert_framed_area(
        bottom.into(),
        top.into(),
        MapPermission::R | MapPermission::W,
    );
    KernelStack(kstack_id)
}

/// 返回内核栈（kstack）的 bottom 和 top 地址，他们之间的关系如图所示：
/// https://res.niuxuewei.com/2022-10-19-090859.png
/// ┌───────────┐◀───bottom
/// │           │
/// │  kstack   │
/// │           │
/// └───────────┘◀───top
fn kernel_stack_position(kstack_id: usize) -> (usize, usize) {
    let top = TRAMPOLINE - kstack_id * (KERNEL_STACK_SIZE + PAGE_SIZE);
    let bottom = top - KERNEL_STACK_SIZE;
    (bottom, top)
}
