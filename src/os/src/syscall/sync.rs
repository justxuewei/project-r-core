use alloc::sync::Arc;

use crate::{
    sync::mutex::{Mutex, MutexBlocking, MutexSpin},
    task::processor::current_process,
};

/// 创建锁
/// blocking 表示锁类型，如果是 true 则说明是阻塞锁（MutexBlocking），否则是自旋
/// 锁（MutexSpin）。
/// 返回值是锁的 ID，在后续加锁、解锁的过程中都需要使用。
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

/// 加锁
pub fn sys_mutex_lock(mutex_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.lock();
    0
}

/// 解锁
pub fn sys_mutex_unlock(mutex_id: usize) -> isize {
    let process = current_process();
    let process_inner = process.inner_exclusive_access();
    let mutex = Arc::clone(process_inner.mutex_list[mutex_id].as_ref().unwrap());
    drop(process_inner);
    drop(process);
    mutex.unlock();
    0
}
