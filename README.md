# Pure-Lang

自然语言编译器 — 将用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

## 项目概览

Pure-Lang 是一个**自然语言编译器**：接收用户的自然语言需求，将其编译为可执行的计划、代码生成意图和后续动作建议。项目采用模块化单体架构，通过 Tauri 2 桌面应用提供交互界面，核心编译引擎基于 Rust 实现。

> 📖 详细设计文档见 [`design/`](./design/) 目录

## 架构

```text
pure-studio               Tauri 2 桌面应用
  │                       React UI + Tauri 命令桥接 + 事件推送
  ▼
pl-core                   核心编译引擎
  │                       turn/session 编排、配置管理、工具审批、
  │                       MCP 动态工具集成、Studio SQLite 持久化、技能系统
  ├────► pl-model         LLM Provider 适配层
  │                       OpenAI 兼容 wire API、SSE 流式、模型元数据
  ├────► pl-lsp           LSP 客户端
  │                       rust-analyzer 支持、代码智能查询
  └────► pl-protocol      公共协议层
                          Agent 事件、消息、权限、错误类型
```

### Workspace Crate

| Crate | 路径 | 职责 |
|-------|------|------|
| `pl-protocol` | `code/pl-protocol/` | 跨 crate 协议类型：消息、事件、错误、权限 |
| `pl-model` | `code/pl-model/` | LLM provider 抽象与适配：OpenAI 兼容 API、SSE 流式、模型元数据管理 |
| `pl-lsp` | `code/pl-lsp/` | LSP 客户端：rust-analyzer 支持、代码智能查询 |
| `pl-core` | `code/pl-core/` | 核心编译引擎：端口-适配器架构，含 `application`、`domain`、`infrastructure`、`interfaces` 四层 |
| `pure-studio` | `code/pure-studio/` | Tauri 2 桌面应用：React 前端 + Rust Tauri 桥接 |

### 依赖规则

```
pl-protocol  ←  pl-model  ←  pl-core  ←  pure-studio
                pl-lsp     ←  pl-core
（底层）                                    （顶层）
```

## 快速开始

### 前置条件

- [Rust](https://rustup.rs/)（edition 2024）
- [Node.js](https://nodejs.org/) LTS
- Windows / macOS / Linux

### 启动 Pure Studio 桌面应用

```powershell
# Windows（一键启动，自动检查依赖）
./run-pure-studio.ps1

# 或手动启动
cd code/pure-studio
npm install
npm run tauri:dev
```

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
│   │   │   ├── command/          # Shell 进程管理（bash + write_stdin）
│   │   │   ├── file/             # 文件操作（read、write、apply_patch）
│   │   │   ├── multi_agent/      # 子代理编排工具
│   │   │   ├── ask_user.rs       # 向用户提问
│   │   │   ├── lsp.rs            # LSP 查询
│   │   │   └── skill.rs          # 技能系统工具
│   │   ├── src/skill/           # 技能目录与扫描
│   │   ├── src/studio/          # Studio 运行时（SQLite、审批）
│   │   └── migrations/          # SeaORM SQLite 迁移
│   └── pure-studio/          # Tauri 2 桌面应用
│       ├── src-tauri/          # Rust 后端（命令桥接、事件、审批）
│       └── src/                # React + TypeScript 前端
├── design/                   # 架构设计文档（14 份）
│   ├── 01-overview.md
│   ├── 02-crates.md
│   ├── 03-pipeline.md
│   ├── ...
│   └── 13-tool-calling-runtime.md
├── .claude/skills/           # 项目技能（Codex 协作规则）
├── .cargo/config.toml        # Cargo 配置
├── CLAUDE.md                 # 项目规范
├── Agents.md                 # Codex 项目记忆
└── run-pure-studio.ps1       # Windows 启动脚本
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Tauri 2 |
| 后端语言 | Rust（edition 2024） |
| 异步运行时 | tokio |
| 数据库 | SQLite via SeaORM（SQLx 后端） |
| 序列化 | serde + serde_json + toml |
| 前端框架 | React 18 + TypeScript |
| 构建工具 | Vite |
| UI 图标 | lucide-react |
| 国际化 | i18next + react-i18next |
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
| **CompileMode** | 编译模式（auto / plan / compact） |

### 内置工具

共 23 个内置工具 + MCP 动态工具，按分类如下：

| 分类 | 工具 |
|------|------|
| Shell | `bash`, `write_stdin` |
| 文件读取 | `read_file`, `list_files`, `search_files`, `stat_path` |
| 文件写入 | `write_file`, `create_directory`, `delete_path`, `copy_path`, `move_path` |
| 补丁 | `apply_patch` |
| 代码智能 | `lsp_query` |
| 子代理 | `spawn_agent`, `wait_agent`, `list_agents`, `send_message`, `followup_task`, `close_agent` |
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
cargo test -p pl-model
cargo test -p pl-core

# 运行全部测试
cargo test --workspace
```

### 前端

```bash
# 类型检查
npm --prefix code/pure-studio run typecheck

# 构建
npm --prefix code/pure-studio run build

# 运行前端测试
npm --prefix code/pure-studio run test
```

### Tauri 开发

```bash
cd code/pure-studio
npm run tauri:dev       # 启动开发模式（热重载）
npm run tauri:build     # 生产构建
```

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

## 项目规范

详细的编码约定和协作规则见：

- [`CLAUDE.md`](./CLAUDE.md) — 项目规范（RPITIT、模块设计、导出、测试等）
- [`Agents.md`](./Agents.md) — 项目记忆与 Codex 协作约定

## License

Apache-2.0
