use core::cell::RefMut;

use alloc::{
    string::String,
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
        page_table::translated_ref_mut,
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

    pub fn exec(&self, elf_data: &[u8], args: Vec<String>) {
        let (mmset, mut user_sp, entrypoint) = MemorySet::from_elf(elf_data);

        let trap_cx_ppn = mmset
            .translate(VirtAddr::from(config::TRAP_CONTEXT).into())
            .unwrap()
            .ppn();

        // push args on user sp
        user_sp -= (args.len() + 1) * core::mem::size_of::<usize>();
        let argv_base = user_sp;
        // user_sp layout for `{app_name}` ab cd
        // <High Addr> | \0 | *argv[1] | *argv[0] | \0 | 'b' | 'a'(**argv[0]) | \0 | 'd' | 'c'(**argv[1]) | <Low Addr>
        // 这一小段处理的是 <High Addr> | \0 | *argv[1] | *argv[0] | <Low Addr>
        // argv[i] 指向的是第 i 个参数的首地址，以 *argv[0] 指向的地址就是 'b' 字符的地址
        let mut argv: Vec<_> = (0..=args.len())
            .map(|arg| {
                translated_ref_mut(
                    mmset.token(),
                    (argv_base + arg * core::mem::size_of::<usize>()) as *mut usize,
                )
            })
            .collect();
        *argv[args.len()] = 0;
        // 复制 args 到 user_sp
        // 这一小段处理的是 <High Addr> | \0 | 'b' | 'a'(**argv[0]) | \0 | 'd' | 'c'(**argv[1]) | <Low Addr>
        for i in 0..args.len() {
            user_sp -= args[i].len() + 1;
            *argv[i] = user_sp;
            let mut p = user_sp;
            for c in args[i].as_bytes() {
                *translated_ref_mut(mmset.token(), p as *mut u8) = *c;
                p += 1;
            }
            *translated_ref_mut(mmset.token(), p as *mut u8) = 0;
        }
        // 内存对齐（符合 k210 平台要求的）
        user_sp -= user_sp % core::mem::size_of::<usize>();

        let mut tcb_inner = self.inner_exclusive_access();
        tcb_inner.memory_set = mmset;
        tcb_inner.trap_cx_ppn = trap_cx_ppn;
        let mut trap_cx = TrapContext::app_init_context(
            entrypoint,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap::trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *tcb_inner.get_trap_cx() = trap_cx;
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum TaskStatus {
    Ready,
    Running,
    Zombie,
}
