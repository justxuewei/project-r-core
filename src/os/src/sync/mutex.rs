use alloc::{collections::VecDeque, sync::Arc};

use crate::task::{
    block_current_and_run_next, manager::add_task, processor::current_task,
    suspend_current_and_run_next, TaskControlBlock,
};

use super::UPSafeCell;

pub trait Mutex: Sync + Send {
    fn lock(&self);
    fn unlock(&self);
}

/// 自旋锁，由于在 rCore 中仅使用一个核心，所以自旋锁在实现过程中不必使用
/// CAS/TAS 原子语句。
pub struct MutexSpin {
    locked: UPSafeCell<bool>,
}

impl MutexSpin {
    pub fn new() -> Self {
        Self {
            locked: unsafe { UPSafeCell::new(false) },
        }
    }
}

impl Mutex for MutexSpin {
    fn lock(&self) {
        loop {
            let mut locked = self.locked.exclusive_access();
            if *locked {
                drop(locked);
                // 主动让出 cpu 使用权
                suspend_current_and_run_next();
                continue;
            } else {
                *locked = true;
                return;
            }
        }
    }

    fn unlock(&self) {
        let mut locked = self.locked.exclusive_access();
        *locked = false;
    }
}

pub struct MutexBlocking {
    inner: UPSafeCell<MutexBlockingInner>,
}

pub struct MutexBlockingInner {
    locked: bool,
    wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl MutexBlocking {
    pub fn new() -> Self {
        Self {
            inner: unsafe {
                UPSafeCell::new(MutexBlockingInner {
                    locked: false,
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }
}

impl Mutex for MutexBlocking {
    fn lock(&self) {
        let mut inner = self.inner.exclusive_access();
        if inner.locked {
            inner.wait_queue.push_back(current_task().unwrap());
            drop(inner);
            block_current_and_run_next();
        } else {
            inner.locked = true;
        }
    }

    fn unlock(&self) {
        let mut inner = self.inner.exclusive_access();
        assert!(inner.locked);
        // 如果有线程等待被唤醒，唤醒被阻塞的线程，如果没有就解锁
        if let Some(task) = inner.wait_queue.pop_front() {
            add_task(task);
        } else {
            inner.locked = false;
        }
    }
}
