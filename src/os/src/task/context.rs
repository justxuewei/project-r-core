use crate::trap::trap_return;

#[derive(Copy, Clone)]
#[repr(C)]
// TaskContext 存放进程切换上下文（主要内容为 callee registers）
pub struct TaskContext {
    // return address
    // TODO(justxuewei): debug
    // ra: usize,
    pub ra: usize,
    // kernel stack
    pub sp: usize,
    // sp: usize,
    // called saved registers
    s: [usize; 12],
}

impl TaskContext {
    pub fn zero_init() -> Self {
        Self {
            ra: 0,
            sp: 0,
            s: [0; 12],
        }
    }

    /// 创建一个 trap_return 的 TaskContext，拥有这个 TaskContext 的线程，在执行
    /// 的时候将会直接返回用户态，具体操作是：
    /// other thread -> trap_return -> user application
    pub fn goto_trap_return(kstack_ptr: usize) -> Self {
        Self {
            ra: trap_return as usize,
            sp: kstack_ptr,
            s: [0; 12],
        }
    }
}
