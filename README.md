# Pure-Lang

自然语言编译器 — 将用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

## 项目概览

Pure-Lang 是一个**自然语言编译器**：接收用户的自然语言需求，将其编译为可执行的计划、代码生成意图和后续动作建议。项目采用模块化单体架构，核心编译引擎基于 Rust 实现；桌面端以 Windows 优先的 Flutter + flutter_rust_bridge 实现为唯一入口。

> 📖 详细设计文档见 [`design/`](./design/) 目录

## 架构

```text
pl-core                   核心编译引擎与 Studio runtime
  │                       turn/session 编排、配置管理、工具审批、
  │                       MCP 动态工具集成、Studio SQLite 持久化、技能系统、
  │                       runtime 状态机、会话事件订阅
  ├────► pl-model         LLM Provider 适配层
  │                       OpenAI 兼容 wire API、SSE 流式、模型元数据
  ├────► pl-lsp           LSP 客户端
  │                       rust-analyzer 支持、代码智能查询
  ├────► pl-protocol      公共协议层
  │                       Studio wire DTO、消息、权限、错误类型
  ├────► pl-trace         内部 trace 事件层
  │                       AgentEvent、TraceEvent、TracePart
  ▲
  │
pl-studio-bridge          Flutter Rust Bridge v2 桥接 crate
  ▲
  │
pure-studio       Flutter Windows 桌面应用
                          Material 3 + Riverpod + 会话级事件流
```

### Workspace Crate

| Crate | 路径 | 职责 |
|-------|------|------|
| `pl-protocol` | `code/pl-protocol/` | 跨 crate 协议类型：消息、事件、错误、权限 |
| `pl-trace` | `code/pl-trace/` | 内部运行事件类型：AgentEvent、TraceEvent、TracePart |
| `pl-model` | `code/pl-model/` | LLM provider 抽象与适配：OpenAI 兼容 API、SSE 流式、模型元数据管理 |
| `pl-lsp` | `code/pl-lsp/` | LSP 客户端：rust-analyzer 支持、代码智能查询 |
| `pl-core` | `code/pl-core/` | 核心编译引擎与 Studio runtime：turn/session、配置、工具、MCP、SQLite projection，并通过 `interfaces` 等模块逐步端口化 |
| `pl-studio-bridge` | `code/pure-studio/rust/` | Flutter Rust Bridge v2 桥接 crate：把 Flutter API 转为 `pl-core` runtime 调用 |
| `pure-studio` | `code/pure-studio/` | Flutter Windows 桌面应用：Material 3、Riverpod、会话级事件订阅 |
| `pl-xtask` | `xtask/` | 本仓库开发任务入口：GUI 验证、运行、发布构建和 Flutter Windows Rust bridge 构建 |

### 依赖规则

```
pl-protocol  ←  pl-trace  ←  pl-model  ←  pl-core  ←  pl-studio-bridge  ←  pure-studio
                              pl-lsp    ←  pl-core
（底层）                                                         （顶层）
```

## 快速开始

### 前置条件

- [Rust](https://rustup.rs/)（edition 2024）
- [Flutter](https://docs.flutter.dev/get-started/install)（Windows 桌面端，`flutter` 需在 PATH 中）
- Windows

### 启动 Pure Studio 桌面应用

```powershell
# Windows（Flutter + flutter_rust_bridge v2）
cargo xtask run-gui
```

Flutter 端通过 `pl-studio-bridge` 调用 `pl-core` Studio runtime。每个打开的会话只订阅自己的高频 timeline/turn/interaction 流；MCP/LSP health、配置和项目列表等低频事件走全局流。

首次启动后，在 Pure Studio 设置页面配置 LLM Provider。配置保存在：

```text
~/.pure/config.toml                 # 全局配置（provider、模型、角色）
~/.pure/studio/studio_*.sqlite      # Studio 数据库（会话、消息、运行时记录）
```

### 项目结构

```
pure-lang/
├── code/
│   ├── pl-protocol/          # 公共协议层
│   ├── pl-model/             # LLM provider 适配
│   ├── pl-lsp/               # LSP 客户端（rust-analyzer 支持）
│   ├── pl-core/              # 核心编译引擎
│   │   ├── src/agent/          # 子代理系统
│   │   ├── src/config/         # 配置系统（provider、role、MCP、runtime）
│   │   ├── src/core/           # 核心引擎（turn loop、tool dispatch、权限）
│   │   ├── src/domain/         # 领域模型
│   │   ├── src/infrastructure/ # 基础设施适配器（SQLite、文件系统）
│   │   ├── src/interfaces/     # 端口定义
│   │   ├── src/mcp/            # MCP 动态工具运行时
│   │   ├── src/tool/           # 工具系统
│   │   │   ├── command/          # Shell 进程管理（exec + write_stdin）
│   │   │   ├── file/             # 文件操作（read、write、apply_patch）
│   │   │   ├── multi_agent/      # 子代理编排工具
│   │   │   ├── ask_user.rs       # 向用户提问
│   │   │   ├── lsp.rs            # LSP 查询
│   │   │   └── skill.rs          # 技能系统工具
│   │   ├── src/skill/           # 技能目录与扫描
│   │   ├── src/studio/          # Studio 运行时（SQLite、审批）
│   │   └── migrations/          # SeaORM SQLite 迁移
│   └── pure-studio/  # Flutter Windows 桌面应用
│       ├── lib/                # Material 3 + Riverpod UI
│       ├── rust/               # pl-studio-bridge crate
│       └── windows/            # Flutter Windows runner
├── design/                   # 架构设计文档（14 份）
│   ├── 01-overview.md
│   ├── 02-crates.md
│   ├── 03-pipeline.md
│   ├── ...
│   └── 13-tool-calling-runtime.md
├── .claude/skills/           # 项目技能（Codex 协作规则）
├── .cargo/config.toml        # Cargo 配置
├── xtask/                    # pl-xtask 开发任务入口
├── CLAUDE.md                 # 项目规范
└── Agents.md                 # Codex 项目记忆
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Flutter Windows + flutter_rust_bridge v2 |
| 后端语言 | Rust（edition 2024） |
| 异步运行时 | tokio |
| 数据库 | SQLite via SeaORM（SQLx 后端） |
| 序列化 | serde + serde_json + toml |
| Flutter 状态管理 | Riverpod |
| Flutter 路由 | go_router |
| LLM 集成 | OpenAI 兼容 API（async-openai + SSE 流式） |
| LSP 客户端 | lsp-types + 自研 JSON-RPC framing（rust-analyzer 支持） |
| 流式解析 | async-openai stream |

## 核心概念

| 概念 | 说明 |
|------|------|
| **Turn** | 单轮编译请求，包含消息、工具调用、模型交互的完整生命周期 |
| **Session** | 多轮会话管理，维护消息历史和上下文 |
| **Tool** | 可执行工具（Shell、文件操作、Subagent、LSP 查询、用户提问等），通过 `ToolRegistry` 注册；支持 MCP 动态工具扩展 |
| **MCP** | Model Context Protocol 集成，运行时发现和调用外部 MCP 服务器提供的工具 |
| **LSP** | Language Server Protocol 客户端，支持代码智能查询（定义跳转、引用查找等） |
| **Agent** | 子代理系统，支持分层任务分解与编排 |
| **Skill** | 项目技能系统，定义 Codex 协作规则和可复用流程 |
| **Studio** | Pure Studio 桌面运行时，管理项目、会话和配置 |
| **Provider** | LLM Provider 抽象（OpenAI、DeepSeek、智谱等） |
| **CompileMode** | 编译模式（auto / plan） |

### 内置工具

内置工具 + MCP 动态工具按分类如下：

| 分类 | 工具 |
|------|------|
| Shell | `exec`, `write_stdin`（内容搜索用 `rg`，文件搜索用 `rg --files`） |
| 文件读取 | `read_file`, `list_files`, `stat_path` |
| 文件写入 | `write_file`, `create_directory`, `delete_path`, `copy_path`, `move_path` |
| 补丁 | `apply_patch` |
| 代码智能 | `lsp_query` |
| 子代理 | `spawn_agent`, `report_progress`, `send_message`, `interrupt_agent`, `list_agents`, `wait_agents`, `read_agent_session`, `close_agent` |
| 用户交互 | `request_user_input` |
| 技能 | `skills_list`, `skill_view`, `skill_manage` |
| MCP | 动态注册（`mcp__<server>__<tool>`） |

## 开发

### Rust 后端

```bash
# 格式化
cargo fmt

# Lint（严格模式）
cargo clippy -- -D warnings

# 运行各 crate 测试
cargo test -p pl-protocol
cargo test -p pl-trace
cargo test -p pl-model
cargo test -p pl-lsp
cargo test -p pl-core
cargo test -p pl-studio-bridge

# 运行全部测试
cargo test --workspace
```

### Flutter 开发

```powershell
# 从仓库根目录执行一般 Flutter/Dart 命令，参数原样透传
cargo flutter analyze
cargo flutter test
cargo dart format lib

# 从仓库根目录解析依赖、静态分析并运行非视觉测试
cargo xtask verify-gui

# 从仓库根目录运行 GUI
cargo xtask run-gui

# 从仓库根目录构建当前 OS 的 release 产物
cargo xtask build-gui
```

Markdown/timeline 视觉检查可以使用本地 demo 数据启动，不连接 runtime：

```powershell
cargo xtask run-gui --demo
```

本仓库要求 Flutter 端使用 `flutter_rust_bridge` v2.12.x；本机 codegen 版本应与 Dart/Rust 依赖保持同一小版本。

## 设计文档

项目完整的架构决策和设计说明收录在 [`design/`](./design/) 目录：

| 文档 | 内容 |
|------|------|
| [01-overview.md](./design/01-overview.md) | 系统总览与定位 |
| [02-crates.md](./design/02-crates.md) | Crate 设计与端口-适配器架构 |
| [03-pipeline.md](./design/03-pipeline.md) | 编译管线流程 |
| [04-security.md](./design/04-security.md) | 安全与权限模型 |
| [05-extension.md](./design/05-extension.md) | 扩展机制 |
| [06-phases.md](./design/06-phases.md) | 编译阶段说明 |
| [07-model.md](./design/07-model.md) | 模型与 Provider 设计 |
| [08-streaming.md](./design/08-streaming.md) | SSE 流式处理 |
| [09-conventions.md](./design/09-conventions.md) | 编码约定 |
| [10-config.md](./design/10-config.md) | 配置系统 |
| [11-studio-ui.md](./design/11-studio-ui.md) | Studio UI 设计 |
| [12-plan-b-implementation-spec.md](./design/12-plan-b-implementation-spec.md) | 方案乙实现规约 |
| [13-skills.md](./design/13-skills.md) | 技能系统设计 |
| [13-tool-calling-runtime.md](./design/13-tool-calling-runtime.md) | 工具调用运行时 |
| [14-lsp-runtime.md](./design/14-lsp-runtime.md) | LSP 运行时 |

## 项目规范

详细的编码约定和协作规则见：

- [`CLAUDE.md`](./CLAUDE.md) — 项目规范（RPITIT、模块设计、导出、测试等）
- [`Agents.md`](./Agents.md) — 项目记忆与 Codex 协作约定

## License

Apache-2.0
