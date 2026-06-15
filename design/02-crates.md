# 02 - Crate 设计（方案乙）

## 2.1 总体形态

本仓库继续保持模块化单体，不新增常驻进程。核心边界采用端口-适配器：

- `pl-protocol`：跨 crate 公共 wire 协议、状态与错误
- `pl-trace`：内部 agent/trace 事件协议
- `pl-model`：模型 provider 适配
- `pl-core`：应用编排、领域模型、端口定义、基础设施适配器
- `pure-studio`：Tauri 桥接与 React UI

## 2.2 pl-protocol

职责保持不变：定义稳定 wire 协议、错误与公共状态类型。

- 放置 `PureError`、`Message`、Studio DTO、interaction、runtime usage、agent status 等跨层共享类型
- 不依赖任何内部 crate
- 不包含 raw `AgentEvent` / `TracePart`、运行时行为与存储实现

## 2.3 pl-trace

`pl-trace` 是内部运行事件 crate。

- 放置 `AgentEvent`、`AgentEventSender`、`TraceEvent`、`TracePart`、`EnabledToolsEvent` 等 core/provider 内部类型
- 依赖 `pl-protocol` 的公共状态与 interaction 类型
- 不作为 Studio wire DTO 暴露；进入 UI 前必须由 `pl-core` 映射为 `StudioEventEnvelope`

## 2.4 pl-model

职责保持不变：封装 provider 差异，不承担会话编排。

- `ModelProvider` / `CompletionRequest` / `CompletionResponse`
- OpenAI-compatible wire 适配
- 依赖 `pl-protocol` 与 `pl-trace`，不依赖 `pl-core`

## 2.5 pl-core（端口-适配器）

`pl-core` 调整为四层目录语义：

- `application`：use case 编排（`StudioRuntime`）
- `domain`：会话、项目、timeline、审批等领域记录类型
- `interfaces`：端口 trait（RPITIT + `Send`）
- `infrastructure`：SQLite、文件系统、事件发射、工具执行等适配器

核心端口（示例）：

- `SessionRepository`
- `ConfigRepository`
- `TraceRepository`
- `EventSink`
- `ToolExecutor`

约束：

- trait 异步方法统一使用原生 RPITIT，并显式 `+ Send`
- `lib.rs` 只做模块声明与 `pub use` 出口
- `StudioRuntime` 不直接嵌入具体数据库/文件系统细节

审批默认策略固定为 `ToolApprovalPolicy::AutoAllow`。手动审批链路保留接口，但不是默认执行路径。

## 2.6 pure-studio（桥接 + UI）

`pure-studio/src-tauri` 采用壳层 main：

- `main.rs`：启动、状态注入、命令注册
- `commands/*`：命令处理
- `dto/*`：命令与事件 DTO
- `events/*`：事件映射与分发
- `approvals/*`：审批等待队列与解析
- `state/*`：共享状态

前端 `src/App.tsx` 改为页面装配壳层，状态迁移到 reducer。

## 2.7 本地数据版本

方案乙采用破坏性升级，不保留运行期兼容层：

- SQLite 切换到新 schema（v2）
- `config.toml` 切换到新结构（v2）
- 启动时检测旧格式：先备份，再重建新结构

## 2.8 Workspace

workspace crate 组成保持不变：

```toml
[workspace]
members = [
    "code/pl-protocol",
    "code/pl-trace",
    "code/pl-model",
    "code/pl-core",
    "code/pure-studio/src-tauri",
]
resolver = "3"
```
