use alloc::{
    collections::{BTreeMap, VecDeque},
    sync::Arc,
};
use lazy_static::*;

use crate::sync::UPSafeCell;

use super::task::TaskControlBlock;

// TaskManager 管理全局需要执行的进程 (TaskControlBlock)，
// 需要和 Processor 相互配合。
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
}

lazy_static! {
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
    pub static ref PID_TO_TASK: UPSafeCell<BTreeMap<usize, Arc<TaskControlBlock>>> =
        unsafe { UPSafeCell::new(BTreeMap::new()) };
}

// 添加一个任务
pub fn add_task(task: Arc<TaskControlBlock>) {
    PID_TO_TASK
        .exclusive_access()
        .insert(task.getpid(), task.clone());
    TASK_MANAGER.exclusive_access().add(task);
}

pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    TASK_MANAGER.exclusive_access().fetch()
}

/// 通过 pid 获取 task control block
pub fn get_task_by_pid(pid: usize) -> Option<Arc<TaskControlBlock>> {
    PID_TO_TASK.exclusive_access().get(&pid).map(Arc::clone)
}

/// 移除 pid 和 task control block 的映射关系
pub fn remove_from_pid_to_task(pid: usize) {
    if PID_TO_TASK.exclusive_access().remove(&pid).is_none() {
        panic!(
            "Can't find task control block from pid_to_tcb map: pid {} not found.",
            pid
        );
    }
}
