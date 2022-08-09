use core::cell::{RefCell, RefMut};

pub struct UPSafeCell<T> {
    inner: RefCell<T>,
}

// Sync 是一个标记性 trait，告诉编译器 UPSafeCell 是可以被多个线程安全访问的。
// 这里 unsafe 表示实现了一个不安全的 trait Sync。
// Ref:
// 1. https://kaisery.github.io/trpl-zh-cn/ch16-04-extensible-concurrency-sync-and-send.html
// 2. https://kaisery.github.io/trpl-zh-cn/ch19-01-unsafe-rust.html
unsafe impl<T> Sync for UPSafeCell<T> {}

impl<T> UPSafeCell<T> {
    // 这里为什么要使用 unsafe？
    // 我理解因为 Self 实现了 unsafe trait: Sync，所以在调用 new 函数的时候也
    // 必须使用 unsafe。
    pub unsafe fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }

    // 这里的 '_ 表示让编译器自动推断生命周期，属于 early bound 形式，
    // 但是这个地方还是有点让人费解，等回头有空的时候再研究下。
    // Ref: https://zhuanlan.zhihu.com/p/404621613
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}
