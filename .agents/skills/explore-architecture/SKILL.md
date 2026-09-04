---
name: explore-architecture
description: Use when asked to understand, document, or summarize the Pure-Lang architecture across its crates, Studio products, and runtime boundaries.
category: guides
platforms: ["windows", "linux", "macos"]
---

# 探索 Pure-Lang 架构

全面理解 Pure-Lang 架构时，先以设计文档建立边界，再按相互独立的模块分派只读子代理，最后沿
`file:line` 小范围复核并汇总。不要用过时的固定目录清单替代当前仓库事实。

## 建立全局视角

主代理必须先完整读取：

- `AGENTS.md`：项目约定、子代理规则和验证入口。
- `design/01-overview.md`：系统定位、核心概念和运行时边界。
- `design/02-crates.md`：crate 职责和依赖方向。
- 根 `Cargo.toml`：当前 workspace members 的唯一事实来源。

按问题补充 `design/03-pipeline.md`、`design/06-phases.md`、
`design/13-tool-calling-runtime.md` 及相关专题设计。架构文档属于判断地基，必须由主代理亲自完整
阅读，不能交给子代理摘要代替。

## 当前稳定边界

- 基础库：`pl-protocol`、`pl-trace`、`pl-output`、`pl-patch`、`pl-skill-core`。
- 模型与语言服务：`pl-model`、`pl-lsp`。
- 核心编排：`pl-core`，拥有 Thread、Turn、工具、Agent、Skill 与通用远程能力。
- 产品运行时：`pl-studio-runtime`，拥有 Studio 状态、存储与产品编排。
- 产品入口：`pl-studio-server` 与 `pl-studio-bridge`。
- 远程执行：`pl-remote-helper`；工程任务入口：`xtask`；Flutter 客户端位于
  `code/pure-studio/`，但不是 Cargo workspace member。

具体依赖必须从各 crate 的 `Cargo.toml` 核验，不把上述分层误写成单一线性链。

## 分派只读探索

只有跨文件、跨目录或会产生大量外围材料的探索才分派。每个子代理任务必须自包含，说明目录、
具体问题、只读约束和期望的 `file:line` 证据。

- 派生时显式传入 `fork_turns = "none"`，省略 `agent_type` 及其他可选执行参数。
- 多个独立问题并发派发，但最多同时使用 10 个子代理。
- 每个子代理只使用一轮，不追派、不复用，也不让子代理继续分派。
- 派发后立即等待本批终态；等待期间不接管已委派范围。
- 只有 `FINAL_ANSWER` 或 completed 状态算终态，过程消息只作为进度信号。

推荐按真实边界而不是机械地“每个 crate 一个代理”拆分，例如基础协议、模型/LSP、核心编排、
Studio 产品、远程运行与前端桥接。范围过大时进一步缩小问题，不让单个代理承担全仓综述。

## 复核与汇总

子代理结果只是压缩后的线索。沿其提供的 `file:line`、符号名和逐字原文抽查关键结论，不重新通读
已经委派的材料。即将修改的确切代码仍由主代理亲自完整读取。

最终汇总至少覆盖：

1. 系统定位与主要用户入口。
2. 当前 workspace members 与非 Cargo 产品目录。
3. crate 职责、公共入口和实际依赖关系。
4. Thread/Turn、模型、工具、Skill、远程与 Studio 的所有权边界。
5. 配置、SQLite 与运行态事实的存储位置。
6. 与问题直接相关的验证入口和仍未核实事项。

发现文档与代码冲突时，明确列出两边证据；涉及长期契约的修改先更新相应 `design/*`。
