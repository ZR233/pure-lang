---
name: rust-api-design
description: 设计、修改、重构或审查 Pure-Lang 的公共或跨 crate Rust 接口、领域类型、类型化错误、trait、模块与 crate 边界、依赖、feature、宏、配置或序列化协议时使用。普通私有实现使用 rust-code-quality；涉及并发或 unsafe 契约时叠加对应技能。
---

# Rust API 设计

## 工作流

1. 先读取根目录 `AGENTS.md`。涉及架构、接口、协议、运行时行为或长期约定时，先找到并更新相应 `design/*`，再实现代码。
2. 完整阅读 [类型与接口](references/types-and-interfaces.md)。
3. 涉及 crate、模块、可见性、feature、依赖或宏时，完整阅读 [模块与依赖](references/modules-and-dependencies.md)。
4. 先确定调用方、契约所有者、有效状态、失败语义、兼容边界和迁移范围，再选择类型与方法。
5. 在同一改动中迁移现有调用方，更新测试与文档，并删除没有长期兼容责任的双轨入口。

公共不只指 `pub`。跨 crate 约定、序列化格式、配置结构、FRB DTO、Dart domain model、provider 协议、工具输入输出和测试运行器输入都属于共享接口。

## 设计基线

- 使用 newtype、enum、经过校验的结构和领域对象表达约束，让非法状态难以构造。
- 让 struct 字段默认私有，通过构造函数与领域方法维护不变量；数据传输对象与领域对象分离。
- 让 trait 小而专注。共享状态使用组合，共享行为使用 trait，封闭变化使用 enum；只有运行期开放扩展确有需要时使用 `dyn Trait`。
- 让所有权、借用、发布、取消和释放责任从类型与方法可判断；避免返回迫使调用方穿透内部结构的引用。
- 让调用方能够类型化地区分成功、暂不可用、不支持和失败；第三方类型在 adapter、repository 或 runtime 边界转换。
- 让模块默认私有，稳定领域边界优先通过 `pub mod` 形成可读命名空间；少量高频入口才精确重导出，模块全部公共项都属于上层稳定 API 时才使用通配重导出。
- 让同一公共接口只有一条 canonical 路径；无明确跨版本责任时，在同一改动中迁移调用方并删除旧导出、代理入口和兼容别名。
- 让已知结构化协议使用 typed DTO、serde 或现有协议类型；动态值只停留在确实开放或异构的 adapter/wire 边界。
- 让依赖方向从低层领域指向抽象，而不是指向高层编排或具体基础设施。
- 让异步公共 trait 遵守项目的原生 RPITIT 与显式 `Send` 约定，不引入 `async_trait`。

## 专项衔接

接口跨线程、异步任务或共享生命周期时使用 `rust-concurrency-safety`。接口要求调用方维护编译器无法检查的内存安全前置条件时使用 `rust-unsafe-safety`。结构化协议变更必须沿 `AGENTS.md` 规定同步 Rust protocol、FRB DTO、Dart model、reducer/projection 和设计文档。

## 验收

调用方无需穿透内部对象、猜测状态或解析诊断字符串即可完成领域操作。类型、错误、feature 组合、重新导出、序列化兼容和依赖方向在受支持配置中一致，并由正确层级的测试覆盖。
