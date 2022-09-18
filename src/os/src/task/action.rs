use super::{SignalFlags, MAX_SIG};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SignalAction {
    // 信号处理函数
    pub handler: usize,
    // 信号掩码
    pub mask: SignalFlags,
}

impl Default for SignalAction {
    fn default() -> Self {
        Self {
            handler: 0,
            // 40 -> 0b00101000: 屏蔽 SIGILL 和 SIGABRT
            // TODO(justxuewei): 为啥要默认屏蔽这两个信号？
            mask: SignalFlags::from_bits(40).unwrap(),
        }
    }
}

// 如果进程想要自定义信号的处理，需要在 SignalActions 中注册，信号和信号处理函数
// 是一一对应的。
#[derive(Clone)]
pub struct SignalActions {
    pub table: [SignalAction; MAX_SIG + 1],
}

impl Default for SignalActions {
    fn default() -> Self {
        Self {
            table: [SignalAction::default(); MAX_SIG + 1],
        }
    }
}
