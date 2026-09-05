//! Studio 对 Thread 领域的产品内置实现。

pub(crate) mod builtin;
pub(crate) mod simple;
pub(crate) mod task;

pub(crate) use builtin::register_builtins;
