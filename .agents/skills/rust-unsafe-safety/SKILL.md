---
name: rust-unsafe-safety
description: 编写、修改、封装、调试或审查 Pure-Lang 中的 unsafe 块、unsafe 函数或 trait、unsafe impl、裸指针、FFI/FRB/系统 API、C 字符串与缓冲区、回调上下文、手工布局或外部资源句柄时使用。
---

# Rust unsafe 健全性

## 工作流

1. 找到所有安全入口、`unsafe` 操作和释放路径，区分调用方必须维护的前置条件与实现内部可以验证的条件。
2. 任何新增或实质改变的 `unsafe` 边界都完整阅读 [契约与证明](references/contracts-and-proofs.md)。
3. 涉及 FFI、Flutter Rust Bridge、系统 API、C ABI、外部缓冲区、回调或 OS handle 时，完整阅读 [FFI 与外部边界](references/ffi-and-foreign-boundaries.md)。
4. 先使用安全类型验证长度、范围、对齐、状态和所有权，再把不可替代操作缩到最小 `unsafe` 块。
5. 为每个块记录本地证明，并用边界与失败测试补充证据；测试、Miri、sanitizer 或静态检查不能替代逐项证明。

## 基本要求

- 块前说明指针来源、有效范围、对齐、初始化、别名、生命周期、线程访问、释放责任和外部状态中实际相关的条件。
- 不写“调用方保证安全”或“已检查”这类无证据注释；指向附近检查、类型保证或上层 `# Safety` 条款。
- 能由实现检查或类型表达的条件不推给调用方。安全包装对所有安全调用都必须健全，包括错误、取消、panic、重复调用和并发路径。
- 每个 `unsafe fn`、`unsafe trait` 及要求调用方维护安全前置条件的接口都提供 `# Safety` 文档。
- 每个 `unsafe impl Send/Sync` 逐字段证明跨线程转移或共享成立；存在锁不自动证明所有字段受保护。
- 不让 panic 穿越不支持 unwind 的 FFI 边界，不从未经验证的外部值构造 Rust enum、引用或字符串。

## 验收

从每个安全入口追踪到全部 `unsafe` 操作，再追踪正常、错误、取消与 panic 路径的最终释放。每项编译器无法验证的前置条件都有可审查证明；测试覆盖空值、零长度、错位、截断、重复释放、并发关闭与错误回滚。
