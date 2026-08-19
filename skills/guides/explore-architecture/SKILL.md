---
name: explore-architecture
description: Use when asked to understand, document, or summarize the Pure-Lang project architecture across all crates. Covers design doc pre-read, per-crate subagent partitioning, subscription-driven coordination, and fallback recovery.
category: guides
platforms: ["windows"]
---

# Explore Pure-Lang Project Architecture

当需要全面理解 Pure-Lang 项目架构、crate 结构或模块职责时使用。核心思路：先读设计文档确定 crate 边界，再为每个 crate 分配 explorer agent 并行探索，最后汇总合成。

## 前置知识

- 所有 crate 在 `code/` 下，workspace members 见根 `Cargo.toml`。
- 当前 workspace members：
  - `code/pl-protocol` — 跨 crate 公共协议
  - `code/pl-trace` — 内部运行事件
  - `code/pl-model` — LLM Provider 运行时
  - `code/pl-lsp` — LSP 客户端
  - `code/pl-core` — 核心编译引擎
  - `code/pure-studio/rust` — FRB 桥接 crate（包名 `pl-studio-bridge`）
- 依赖方向：`pl-protocol` ← `pl-trace` ← `pl-model` ← `pl-core` ← `pl-studio-bridge`
- Flutter 前端在 `code/pure-studio/`（Dart 包名 `pure_studio`）

## 探索步骤

### 1. 预读设计文档（必须前置）

在分配任何子代理之前，先读取关键设计文档以理解 crate 边界和架构约束：

```powershell
# 最少读取：
design/01-overview.md   # 系统定位、核心概念、依赖规则
design/02-crates.md     # 每个 crate 的职责和边界
```

可选补充（取决于任务深度）：
- `design/03-pipeline.md` — 编译流程
- `design/06-phases.md` — 实施阶段和验证命令
- `design/13-tool-calling-runtime.md` — 工具系统

同时读取 `Cargo.toml` 确认 workspace members 列表。

### 2. 按 crate 分配只读子代理

为每个 crate 创建一个 `spawn_agent`，使用 `agent_type: "default"` 和
`fork_turns: "none"`。每个 agent 的任务应包含：

- 明确的目录路径
- 需要回答的具体问题（目录结构、lib.rs 导出、关键类型、模块职责）
- 不修改文件的约束

**标准分区方案**（6 个 crate，可并行启动）：

| Agent | Scope | 关键问题 |
|-------|-------|----------|
| 1 | `code/pl-protocol` | 公共类型、module 结构、serde 约定 |
| 2 | `code/pl-trace` | AgentEvent/TracePart、与 pl-protocol 关系 |
| 3 | `code/pl-model` | model/provider/completion/runtime 四层、catalog、ResolvedModelRoute 与单模型 ModelRuntime |
| 4 | `code/pl-lsp` | LSP client、语言服务器管理、查询能力 |
| 5 | `code/pl-core` | 所有模块、领域模型、Studio 运行时、SQLite 存储、工具系统 |
| 6 | `code/pure-studio/rust` | FRB API 表面、事件订阅、brige 函数 |

**关于 `pl-core` 的特殊说明**：该 crate 最大、最复杂。其 explorer agent 可能会自己再分出一个子 explorer 探索目录结构。这是预期行为，父 agent 会负责汇总。

### 3. 订阅式收集结果

在同一轮启动所有独立 agent 后立即等待，直到本批结果返回，再沿它们给出的 `file:line` 做小范围
复核；不要在等待期间同时修改文件：

```rust
spawn_all_agents();
wait_for_agent_batch();
verify_reported_locations();
```

### 4. 回退策略（重要）

当 agent 状态为 `shutdown` 且 `summary` 为 `null` 时（`pl-core` 父 agent 可能发生）：

1. 检查该 agent 是否创建了子 agent（通过 `list_agents` 查看路径层级）
2. 读取子 agent 的 `summary`（子 agent 通常状态为 `completed`）
3. 手动读取关键文件作为补充：
   - `code/pl-core/src/lib.rs` — 获取完整导出列表
   - `list_files(depth: 2)` 查看 `code/pl-core/src/` 的模块结构
   - 按需补充读取设计文档

### 5. 清理已完成的 agent

工具提供关闭能力时，汇总完成后关闭所有已完成 agent；否则不复用本轮探子。

## 输出格式

最终架构汇总应包含：

1. **整体定位** — 项目是做什么的
2. **依赖链** — crate 间依赖方向
3. **每个 crate 详解** — 路径、依赖、职责、核心导出、模块组织
4. **核心架构模式** — 端口-适配器、事件驱动、编译流程、工具系统
5. **数据存储** — SQLite、配置格式
6. **实施阶段** — P0/P1/P2 进度

## 常见问题

### `pl-core` 探索不完整
如果 `pl-core` agent 未能返回完整摘要，手动读取其 `lib.rs` 的 `pub use` 列表是最快获取关键导出的方式。配合 `list_files(depth: 2)` 可还原模块结构。

### 无活动超时
无活动超时由 runtime 按 direct child 独立管理。收到 timeout continuation 后，用附带的全部
direct-child snapshots 判断是追问、停止还是继续等待；不要用短轮询，也不要重新创建仍在运行的 agent。

### Agent 数量限制
如果 provider 或环境限制并发 agent 数量（收到 capacity error），可分批启动：先探索 `pl-protocol` + `pl-trace` + `pl-model` 三个较小的 crate，再探索 `pl-lsp` + `pl-core` + bridge。
