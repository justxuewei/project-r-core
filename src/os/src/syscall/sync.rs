use alloc::sync::Arc;

use crate::{
    sync::mutex::{Mutex, MutexBlocking, MutexSpin},
    task::processor::current_process,
};

/// 创建锁
pub fn sys_mutex_create(blocking: bool) -> isize {
    let process = current_process();
    let mutex: Option<Arc<dyn Mutex>> = if blocking {
        Some(Arc::new(MutexBlocking::new()))
    } else {
        Some(Arc::new(MutexSpin::new()))
    };
    let mut process_inner = process.inner_exclusive_access();
    if let Some(id) = process_inner
        .mutex_list
        .iter()
        .enumerate()
        .find(|(_, v)| v.is_none())
        .map(|(i, _)| i)
    {
        process_inner.mutex_list[id] = mutex;
        id as isize
    } else {
        process_inner.mutex_list.push(mutex);
        (process_inner.mutex_list.len() - 1) as isize
    }
}
