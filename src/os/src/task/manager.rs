use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use lazy_static::*;

use crate::{sync::UPSafeCell, task::process::ProcessControlBlock};

use super::task::TaskControlBlock;

// TaskManager 管理全局需要执行的进程 (TaskControlBlock)，负责提供下一个可以执行
// 的任务或者增加/删除任务。
// Processor 与 TaskManager 的关系参见 Processor 注释。
pub struct TaskManager {
    ready_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl TaskManager {
    pub fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
        }
    }

    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        self.ready_queue.push_back(task)
    }

    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        self.ready_queue.pop_front()
    }

    pub fn remove(&mut self, task: Arc<TaskControlBlock>) {
        if let Some((id, _)) = self
            .ready_queue
            .iter()
            .enumerate()
            .find(|(_, t)| Arc::as_ptr(t) == Arc::as_ptr(&task))
        {
            self.ready_queue.remove(id);
        }
    }
}

lazy_static! {
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
    pub static ref PID_TO_PCB: UPSafeCell<BTreeMap<usize, Arc<ProcessControlBlock>>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

/// 添加一个任务
pub fn add_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().add(task);
}

/// 获取一个任务
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}

pub fn remove_task(task: Arc<TaskControlBlock>) {
    TASK_MANAGER.exclusive_access().remove(task)
}

/// 通过 pid 获取 pcb, aka "pid2process"
pub fn get_pcb_by_pid(pid: usize) -> Option<Arc<ProcessControlBlock>> {
    PID_TO_PCB.exclusive_access().get(&pid).map(Arc::clone)
}

/// 移除 pid 和 pcb 的映射关系, aka "remove_from_pid2process"
pub fn remove_from_pid_to_pcb(pid: usize) {
    if PID_TO_PCB.exclusive_access().remove(&pid).is_none() {
        panic!(
            "Can't find task control block from pid_to_pcb map: pid {} not found.",
            pid
        );
    }
}

/// 新增 pid 和 pcb 的映射关系，aka "insert_into_pid2process"
pub fn insert_into_pid_to_pcb(pid: usize, pcb: Arc<ProcessControlBlock>) {
    let map = PID_TO_PCB.exclusive_access();
    if map.get(&pid).is_some() {
        panic!("pid existed");
    }
    map.insert(pid, pcb);
}
