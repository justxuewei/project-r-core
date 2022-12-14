use core::cell::RefMut;

use alloc::{
    string::String,
    sync::{Arc, Weak},
    vec::Vec,
};

use super::{
    id::{pid_alloc, PidHandle, RecycleAllocator},
    manager::{add_task, insert_into_pid_to_pcb},
    task::TaskControlBlock,
    SignalFlags,
};

use crate::{
    fs::{File, Stdin, Stdout},
    mm::{memory_set::MemorySet, page_table::translated_ref_mut, KERNEL_SPACE},
    sync::{mutex::Mutex, UPSafeCell},
    trap::{self, trap_handler, TrapContext},
};

pub struct ProcessControlBlock {
    // immutable
    pub pid: PidHandle,
    // mutable
    inner: UPSafeCell<ProcessControlBlockInner>,
}

pub struct ProcessControlBlockInner {
    pub is_zombie: bool,
    pub memory_set: MemorySet,

    pub parent: Option<Weak<ProcessControlBlock>>,
    pub children: Vec<Arc<ProcessControlBlock>>,

    pub exit_code: i32,

    pub fd_table: Vec<Option<Arc<dyn File + Send + Sync>>>,

    pub signals: SignalFlags,

    pub tasks: Vec<Option<Arc<TaskControlBlock>>>,
    pub task_res_allocator: RecycleAllocator,

    pub mutex_list: Vec<Option<Arc<dyn Mutex>>>,
}

impl ProcessControlBlockInner {
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }

    pub fn alloc_fd(&mut self) -> usize {
        if let Some(fd) = (0..self.fd_table.len()).find(|fd| self.fd_table[*fd].is_none()) {
            fd
        } else {
            self.fd_table.push(None);
            self.fd_table.len() - 1
        }
    }

    pub fn alloc_tid(&mut self) -> usize {
        self.task_res_allocator.alloc()
    }

    pub fn dealloc_tid(&mut self, tid: usize) {
        self.task_res_allocator.dealloc(tid)
    }

    pub fn get_task(&self, tid: usize) -> Arc<TaskControlBlock> {
        self.tasks[tid].as_ref().unwrap().clone()
    }

    pub fn thread_count(&self) -> usize {
        self.tasks.len()
    }
}

impl ProcessControlBlock {
    pub fn inner_exclusive_access(&self) -> RefMut<'_, ProcessControlBlockInner> {
        self.inner.exclusive_access()
    }

    // new 读取用户 elf 程序，创建用户空间同时初始化 kernel stack
    pub fn new(elf_data: &[u8]) -> Arc<Self> {
        let (memory_set, ustack_base, entrypoint) = MemorySet::from_elf(elf_data);
        let token = memory_set.token();

        let pid_handle = pid_alloc();
        let process_inner = unsafe {
            UPSafeCell::new(ProcessControlBlockInner {
                is_zombie: false,
                memory_set,
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
                signals: SignalFlags::empty(),
                tasks: Vec::new(),
                task_res_allocator: RecycleAllocator::new(),
                mutex_list: Vec::new(),
            })
        };

        let process = Arc::new(Self {
            pid: pid_handle,
            inner: process_inner,
        });

        let task = Arc::new(TaskControlBlock::new(process.clone(), ustack_base, true));

        let task_inner = task.inner_exclusive_access();
        // 初始化 TrapContext
        *task_inner.get_trap_cx() = TrapContext::app_init_context(
            entrypoint,
            task_inner.res.as_ref().unwrap().ustack_top(),
            token,
            task.kstack.get_top(),
            trap_handler as usize,
        );
        drop(task_inner);
        // 将 task 加入 process
        let mut process_inner = process.inner_exclusive_access();
        process_inner.tasks.push(Some(task.clone()));
        // 增加 pid to process 的映射关系
        insert_into_pid_to_pcb(process.getpid(), process.clone());
        // 将 task 加入调度队列
        add_task(task);
        drop(process_inner);

        process
    }

    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// 复制进程，目前只支持复制单个 task 的进程
    pub fn fork(self: &Arc<ProcessControlBlock>) -> Arc<ProcessControlBlock> {
        let mut parent_inner = self.inner_exclusive_access();
        if parent_inner.tasks.len() > 1 {
            panic!("too much tasks to fork");
        }
        // 申请新的 pid
        let pid_handle = pid_alloc();
        // 复制内存
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        // 复制 fd 表
        let mut new_fd_table: Vec<Option<Arc<dyn File + Send + Sync>>> = Vec::new();
        for fd in parent_inner.fd_table.iter() {
            if let Some(file) = fd {
                new_fd_table.push(Some(file.clone()));
            } else {
                new_fd_table.push(None);
            }
        }
        // 创建 child 进程
        let child_inner = ProcessControlBlockInner {
            is_zombie: false,
            memory_set,
            parent: Some(Arc::downgrade(self)),
            children: Vec::new(),
            exit_code: 0,
            fd_table: new_fd_table,
            signals: SignalFlags::empty(),
            tasks: Vec::new(),
            task_res_allocator: RecycleAllocator::new(),
            mutex_list: Vec::new(),
        };
        let child = Arc::new(ProcessControlBlock {
            pid: pid_handle,
            inner: unsafe { UPSafeCell::new(child_inner) },
        });
        // 将子进程关联到父进程中
        parent_inner.children.push(child.clone());
        // 创建子进程的主线程，注意这里沿用了 parent 的主线程的 ustack 和 TrapContext
        let task = Arc::new(TaskControlBlock::new(
            child.clone(),
            parent_inner
                .get_task(0)
                .as_ref()
                .inner_exclusive_access()
                .res
                .as_ref()
                .unwrap()
                .ustack_base,
            false,
        ));
        let mut child_inner = child.inner_exclusive_access();
        child_inner.tasks.push(Some(task.clone()));
        drop(child_inner);
        // 修改 TrapContext
        // 这里不能直接继承父进程的主线程的 kstack 吗？
        // 不可以！因为在内核态统一使用的是**操作系统的内存地址空间**（与之对比
        // 的是 ustack、用户内存空间），每一个线程必须要自己申请自己 kstack！
        let task_inner = task.inner_exclusive_access();
        let trap_cx = task_inner.get_trap_cx();
        trap_cx.kernel_sp = task.kstack.get_top();
        drop(task_inner);
        // 增加 pid to process 的映射关系
        insert_into_pid_to_pcb(child.getpid(), child.clone());
        // 将 task 加入调度队列
        add_task(task);

        child
    }

    pub fn exec(&self, elf_data: &[u8], args: Vec<String>) {
        let process_inner = self.inner_exclusive_access();
        assert_eq!(process_inner.thread_count(), 1);
        let (mmset, ustack_base, entrypoint) = MemorySet::from_elf(elf_data);
        let token = mmset.token();

        self.inner_exclusive_access().memory_set = mmset;

        let task = process_inner.get_task(0);
        let mut task_inner = task.inner_exclusive_access();
        let res = task_inner.res.as_mut().unwrap();
        res.ustack_base = ustack_base;
        res.alloc_user_res();
        drop(res);
        task_inner.trap_cx_ppn = task_inner.res.as_mut().unwrap().trap_cx_ppn();

        let mut user_sp = task_inner.res.as_mut().unwrap().ustack_top();
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
                    token,
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
                *translated_ref_mut(token, p as *mut u8) = *c;
                p += 1;
            }
            *translated_ref_mut(token, p as *mut u8) = 0;
        }
        // 内存对齐（符合 k210 平台要求的）
        user_sp -= user_sp % core::mem::size_of::<usize>();

        let mut trap_cx = TrapContext::app_init_context(
            entrypoint,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            task.kstack.get_top(),
            trap::trap_handler as usize,
        );
        trap_cx.x[10] = args.len();
        trap_cx.x[11] = argv_base;
        *task_inner.get_trap_cx() = trap_cx;
    }
}
