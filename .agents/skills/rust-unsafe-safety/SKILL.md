---
name: rust-unsafe-safety
description: 编写、修改、封装或审查 Rust unsafe、unsafe trait/impl、裸指针、FFI、手工布局及外部资源句柄时使用。在 Pure-Lang 中包括 FRB、系统 API、C 字符串、缓冲区和回调上下文的安全边界。
---

# Rust 非安全代码健全性

本技能关注编译器无法证明、必须由实现者或调用方维护的内存安全前置条件。安全函数内部的非安全操作同样需要证明。

## 按需阅读

- 新增或改变非安全边界：完整阅读[契约与证明](references/contracts-and-proofs.md)。
- FFI、FRB、系统 API、外部缓冲区、回调或资源句柄：同时完整阅读[FFI 与外部边界](references/ffi-and-foreign-boundaries.md)。
- 证明涉及同步、取消或跨线程共享：同时使用 `rust-concurrency-safety`。

## 责任与证明

`unsafe` 块保持最小，在附近说明本次操作的指针来源、范围、对齐、初始化、别名、生命周期、线程访问和释放责任中实际相关的条件。不能只写“调用方保证安全”或“已经检查”。

`unsafe fn` 的 `# Safety` 规定调用方义务；`unsafe trait` 的 `# Safety` 规定实现者义务，`unsafe impl` 说明如何履行。trait 的安全方法不能把额外安全义务转给调用方；需要调用方承担无法检查的条件时，方法自身声明为 `unsafe fn`。能由实现检查或由类型保证的条件留在实现侧。

## 验收

从每个安全入口追踪正常、错误、取消、panic 与并发路径，直到最终释放。实质改变安全契约时，由具备相应能力的独立审查者核对证明；这是技术验证，不另设人工授权流程，也不授予提交或发布权限。

测试按 `test-quality` 从合法入口验证行为，不能故意违反 unsafe 调用前置条件制造未定义行为。Miri、sanitizer、模型检查和测试只能补充证据，不能替代安全证明。
