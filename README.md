# Pure-Lang

自然语言编译器 — 将用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

## 项目概览

Pure-Lang 是一个**自然语言编译器**：接收用户的自然语言需求，将其编译为可执行的计划、代码生成意图和后续动作建议。项目采用模块化单体架构，核心编译引擎基于 Rust 实现；桌面端以 Windows 优先的 Flutter + flutter_rust_bridge 实现为唯一入口。

> 📖 详细设计文档见 [`design/`](./design/) 目录

## 架构

```text
pl-core                   产品无关的 Thread/Turn/Item runtime
  │                       模型采样、工具、MCP、LSP 与 agent 编排
  ├────► pl-model         LLM Provider 适配层
  │                       OpenAI 兼容 wire API、SSE 流式、模型元数据
  ├────► pl-lsp           LSP 客户端
  │                       rust-analyzer 支持、代码智能查询
  ├────► pl-protocol      公共协议层
  │                       Studio wire DTO、消息、权限、错误类型
  ├────► pl-trace         内部 trace 事件层
  │                       AgentEvent、TraceEvent、TracePart
  ▼
pl-studio-runtime         Studio SQLite、Task、设置与产品事件
  ├────► pl-studio-server 独立 HTTP/OpenAPI/SSE 宿主
  └────► pl-studio-bridge Flutter Rust Bridge v2 适配器
                 ▲
                 │
pure-studio              Flutter 桌面应用
                         Material 3 + Riverpod + Thread 事件流
```

### Workspace Crate

| Crate | 路径 | 职责 |
|-------|------|------|
| `pl-protocol` | `code/pl-protocol/` | 跨 crate 协议类型：消息、事件、错误、权限 |
| `pl-trace` | `code/pl-trace/` | 内部运行事件类型：AgentEvent、TraceEvent、TracePart |
| `pl-model` | `code/pl-model/` | LLM provider 抽象与适配：OpenAI 兼容 API、SSE 流式、模型元数据管理 |
| `pl-lsp` | `code/pl-lsp/` | LSP 客户端：rust-analyzer 支持、代码智能查询 |
| `pl-output` | `code/pl-output/` | 工具输出截断与模型可见投影算法 |
| `pl-patch` | `code/pl-patch/` | apply-patch 语法、匹配与 backend 契约 |
| `pl-skill-core` | `code/pl-skill-core/` | Skill frontmatter 与路径安全规则 |
| `pl-core` | `code/pl-core/` | 产品无关的 Thread runtime、模型与工具编排、MCP 和 agent runtime |
| `pl-studio-runtime` | `code/pl-studio-runtime/` | Studio SQLite、Task、配置、恢复与产品事件的唯一业务 façade |
| `pl-studio-server` | `code/pl-studio-server/` | 独立 HTTP/OpenAPI/SSE transport 宿主 |
| `pl-studio-bridge` | `code/pure-studio/rust/` | Flutter Rust Bridge v2 transport 适配器 |
| `pure-studio` | `code/pure-studio/` | Flutter 桌面应用：Material 3、Riverpod、Thread 事件订阅 |
| `pl-xtask` | `xtask/` | GUI 生成、验证、运行、构建与发布编排入口 |

### 依赖规则

```
pl-protocol ← pl-trace ← pl-model ← pl-core ← pl-studio-runtime
                          pl-lsp  ↗                 ├→ pl-studio-server
                                                  └→ pl-studio-bridge ← pure-studio
（底层）                                                                    （顶层）
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

Flutter 端通过 `pl-studio-bridge` 调用同一个 `pl-studio-runtime`。每个打开的 Thread 只订阅自己的高频 Item/Turn/interaction 流；MCP/LSP health、配置和项目列表等低频事件走全局产品流。

首次启动后，在 Pure Studio 设置页面配置 LLM Provider。配置保存在：

```text
~/.pure/config.toml                 # 全局配置（provider、模型、角色）
~/.pure/studio/studio.sqlite        # Studio 的单一 canonical 数据库
```

### 项目结构

```
pure-lang/
├── code/
│   ├── pl-protocol/          # 公共协议层
│   ├── pl-trace/             # 内部 trace 事件
│   ├── pl-model/             # LLM provider 适配
│   ├── pl-lsp/               # LSP 客户端（rust-analyzer 支持）
│   ├── pl-output/            # 输出截断算法
│   ├── pl-patch/             # apply-patch 引擎
│   ├── pl-skill-core/        # Skill 核心规则
│   ├── pl-core/              # Thread/Turn/Item 与工具 runtime
│   ├── pl-studio-runtime/    # Studio 业务 runtime
│   ├── pl-studio-server/     # HTTP/OpenAPI/SSE server
│   └── pure-studio/          # Flutter 桌面应用与 FRB crate
├── design/                   # 21 份顶层架构设计文档及原型/视觉资产
├── .cargo/config.toml        # Cargo 配置
├── xtask/                    # pl-xtask 开发任务入口
└── AGENTS.md                 # 项目协作与工程规范
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
| **Thread** | 一个 agent 独占的对话、输入队列与持久历史 |
| **Turn** | Thread 中一次由明确输入启动的执行 |
| **Item** | 消息、推理、工具调用、计划等穷尽的 Thread 内容单元 |
| **Tool** | 可执行工具（Shell、文件操作、Subagent、LSP 查询、用户提问等），通过 `ToolRegistry` 注册；支持 MCP 动态工具扩展 |
| **LSP** | Language Server Protocol 客户端，支持代码智能查询（定义跳转、引用查找等） |
| **Agent** | 子代理系统，支持分层任务分解与编排 |
| **Skill** | 项目技能系统，定义 Codex 协作规则和可复用流程 |
| **Studio** | Pure Studio 桌面运行时，管理项目、会话和配置 |
| **Provider** | LLM Provider 抽象（OpenAI、DeepSeek、智谱等） |
| **StudioMode** | root Thread 模式（Simple / Task） |

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
# 与 CI 一致的 Rust 门禁
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
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
| [15-agent-worktrees.md](./design/15-agent-worktrees.md) | Agent worktree 所有权与生命周期 |
| [16-task-orchestration.md](./design/16-task-orchestration.md) | Simple/Task 编排与状态机 |
| [17-agent-runtime-host.md](./design/17-agent-runtime-host.md) | Agent runtime 与宿主边界 |
| [18-studio-release-update.md](./design/18-studio-release-update.md) | Studio 发布与更新 |
| [19-studio-storage-and-diagnostics.md](./design/19-studio-storage-and-diagnostics.md) | Studio 存储与诊断 |
| [20-studio-state-runtime.md](./design/20-studio-state-runtime.md) | Studio 状态查询与领域生命周期 |

## 项目规范

详细的编码约定和协作规则见 [`AGENTS.md`](./AGENTS.md)。

## License

Apache-2.0
