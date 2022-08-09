use core::cell::RefMut;

use alloc::{
    sync::{Arc, Weak},
    vec::Vec,
};

use super::{
    pid::{self, KernelStack, PidHandle},
    TaskContext,
};

use crate::{
    config,
    fs::{File, Stdin, Stdout},
    mm::{
        self,
        address::{PhysPageNum, VirtAddr},
        memory_set::MemorySet,
        KERNEL_SPACE,
    },
    sync::UPSafeCell,
    trap::{self, trap_handler, TrapContext},
};

pub struct TaskControlBlock {
    // immutable
    pub pid: PidHandle,
    pub kernel_stack: KernelStack,
    // mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

pub struct TaskControlBlockInner {
    pub trap_cx_ppn: PhysPageNum,
    pub base_size: usize,
    pub task_status: TaskStatus,
    pub task_cx: TaskContext,
    pub memory_set: MemorySet,

    pub parent: Option<Weak<TaskControlBlock>>,
    pub children: Vec<Arc<TaskControlBlock>>,

    pub exit_code: i32,

    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,
}

impl TaskControlBlockInner {
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }
}

impl TaskControlBlock {
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }

    // new 读取用户 elf 程序，创建用户空间同时初始化 kernel stack
    pub fn new(elf_data: &[u8]) -> Self {
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(config::TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;

        let pid_handle = pid::pid_alloc();
        let kernel_stack = pid::KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();
        let task_cx_block_inner = unsafe {
            UPSafeCell::new(TaskControlBlockInner {
                task_status,
                task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                memory_set,
                trap_cx_ppn,
                base_size: user_sp,
                parent: None,
                children: Vec::new(),
                exit_code: 0,
                fd_table: vec![
                    // 0 -> stdin
                    Some(Arc::new(Stdin)),
                    // 1 -> stdout
                    Some(Arc::new(Stdout)),
                    // 2 -> stderr
                    Some(Arc::new(Stdout)),
                ],
            })
        };

        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: task_cx_block_inner,
        };

        // init trap context
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            mm::KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );

        task_control_block
    }

    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    pub fn fork(self: &Arc<TaskControlBlock>) -> Arc<TaskControlBlock> {
        let mut parent_inner = self.inner_exclusive_access();

        let pid_handle = pid::pid_alloc();
        let kernel_stack = pid::KernelStack::new(&pid_handle);
        let kernel_stack_top = kernel_stack.get_top();

        // tcb inner
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(config::TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        // copy fd table
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent_inner.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        let tcb_inner = TaskControlBlockInner {
            trap_cx_ppn,
            base_size: parent_inner.base_size,
            task_status: TaskStatus::Ready,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            parent: Some(Arc::downgrade(self)),
            children: Vec::new(),
            exit_code: 0,
            fd_table: new_fd_table,
        };

        let tcb = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe { UPSafeCell::new(tcb_inner) },
        });

        parent_inner.children.push(tcb.clone());

        let mut trap_cx = tcb.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;

        tcb
    }

    pub fn exec(&self, elf_data: &[u8]) {
        let (mmset, user_sp, entrypoint) = MemorySet::from_elf(elf_data);

        let trap_cx_ppn = mmset
            .translate(VirtAddr::from(config::TRAP_CONTEXT).into())
            .unwrap()
            .ppn();
        let mut tcb_inner = self.inner_exclusive_access();
        tcb_inner.memory_set = mmset;
        tcb_inner.trap_cx_ppn = trap_cx_ppn;
        let trap_cx = tcb_inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entrypoint,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap::trap_handler as usize,
        );
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Zombie,
}
