# Pure-Lang

自然语言编译器 — 将用户的自然语言需求整理为可执行导向的编译计划、代码生成意图和后续动作建议。

## 项目概览

Pure-Lang 是一个**自然语言编译器**：接收用户的自然语言需求，将其编译为可执行的计划、代码生成意图和后续动作建议。项目采用模块化单体架构，核心编译引擎基于 Rust 实现；提供 Windows 优先的 Flutter + flutter_rust_bridge 桌面端，以及共享业务运行时的独立 HTTP 宿主。

> 📖 详细设计文档见 [`design/`](./design/) 目录

## 架构

```text
pure_studio → pl-studio-bridge ─┐
                               ├→ pl-studio-runtime → pl-core
HTTP → pl-studio-server ───────┘         │              ↑
                                        ├→ pl-model ───┤
                                        ├→ pl-tool ────┤
                                        ├→ pl-trace ───┘
                                        └→ pl-protocol
```

箭头表示依赖方向。core 定义独立的 Thread、Model/Tool 契约、上下文与可选存储；
model/tool 实现核心接口且不相互依赖。Studio 拥有配置、项目、Mode/Plan/workflow、
子代理协调和统一装配；FRB 与 HTTP 适配同一业务 runtime。

### Workspace 与 Flutter 客户端

| 包 | 路径 | 职责 |
|-------|------|------|
| `pl-protocol` | `code/pl-protocol/` | 跨 crate 协议类型：消息、事件、错误、权限 |
| `pl-trace` | `code/pl-trace/` | 通用 Thread 日志的只读诊断和用量投影 |
| `pl-model` | `code/pl-model/` | LLM provider 抽象与适配：OpenAI 兼容 API、SSE 流式、模型元数据管理 |
| `pl-lsp` | `code/pl-lsp/` | LSP 客户端：rust-analyzer 支持、代码智能查询 |
| `pl-output` | `code/pl-output/` | 工具输出截断与模型可见投影算法 |
| `pl-patch` | `code/pl-patch/` | apply-patch 语法、匹配与 backend 契约 |
| `pl-skill-core` | `code/pl-skill-core/` | Skill frontmatter 与路径安全规则 |
| `pl-core` | `code/pl-core/` | 产品无关的 Thread、模型/工具端口、上下文、不可变日志与可选 SQLite |
| `pl-tool` | `code/pl-tool/` | 文件、命令、SSH、Git、LSP、MCP、Skill、搜索与交互等工具实现 |
| `pl-remote-helper` | `code/pl-remote-helper/` | Linux 本地进程监督与 SSH 远端助手，共用物理进程协议 |
| `pl-studio-runtime` | `code/pl-studio-runtime/` | Studio 产品 SQLite、项目、配置、恢复与产品事件的唯一业务 façade |
| `pl-studio-server` | `code/pl-studio-server/` | 独立 HTTP/OpenAPI/SSE transport 宿主 |
| `pl-studio-bridge` | `code/pure-studio/rust/` | Flutter Rust Bridge v2 transport 适配器 |
| `pure_studio`（非 Cargo 成员） | `code/pure-studio/` | Flutter 桌面应用：Material 3、Riverpod、Thread 事件订阅 |
| `pl-xtask` | `xtask/` | GUI 生成、验证、运行、构建与发布编排入口 |

### 依赖规则

core 不依赖 model、tool、trace、protocol、output 或 Studio，包括测试依赖。
Provider 配置与目录从 model 导入，具体工具从 tool 导入，产品 wire 从 protocol 导入；
不经过 core 转发。tool 组合 LSP、输出、patch、Skill 与远程 helper 等物理能力。

core 默认纯内存，SQLite 通过 `sqlite` feature 显式启用。独立用法见
[`minimal_thread.rs`](./code/pl-core/examples/minimal_thread.rs)：实现 ModelSession 和 Tool，
注册后执行模型/工具循环，并纯重放不透明日志。

## 快速开始

### 前置条件

- [Rust](https://rustup.rs/)（建议使用 stable；项目使用 2024 版语言标准）
- [Flutter](https://docs.flutter.dev/get-started/install) 3.47.1 或兼容版本（`flutter` 需在 PATH 中，内含 Dart）
- Git、PowerShell（Windows）或 Bash（Linux/macOS）
- 一个受支持的桌面开发环境：Windows、Linux 或 macOS

`cargo xtask` 是本仓库统一的构建入口。不要直接在 Flutter 或 Rust 的安装目录执行项目命令；
所有命令都应从仓库根目录运行。

### 编译环境准备

#### 所有桌面系统都需要的 Rust 目标

普通的桌面构建会同时嵌入两个 Linux SSH 远程助手，因此首次准备时安装这两个 Rust 目标：

```powershell
rustup target add aarch64-unknown-linux-musl x86_64-unknown-linux-musl
rustup component add rustfmt clippy
```

生成 Flutter/Rust 桥接代码还需要与仓库依赖一致的代码生成器：

```powershell
cargo install flutter_rust_bridge_codegen --version 2.12.0 --locked
```

远程助手推荐使用 [Zig](https://ziglang.org/) 与
[`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) 交叉编译。Zig 同时提供目标架构的编译器、链接器和 `musl` 系统库，
不需要分别寻找 `aarch64-linux-musl-gcc` 与 `x86_64-linux-musl-gcc`：

```powershell
# Windows：安装 Zig 后重新打开终端，使 PATH 生效
winget install --id zig.zig --exact --accept-source-agreements --accept-package-agreements
cargo install cargo-zigbuild --locked
```

如果本机已有 Python，也可以用一条命令安装 `cargo-zigbuild` 及其 Zig 依赖：

```powershell
py -m pip install --upgrade cargo-zigbuild
```

安装后请确认 `zig version` 与 `cargo zigbuild --version` 均能执行。

Linux/macOS 请从 [Zig 官方下载页](https://ziglang.org/download/) 安装与本机架构匹配的版本，
再执行相同的 `cargo install` 命令。项目会在检测到 `cargo-zigbuild` 位于 PATH，且 `zig` 位于
PATH 或由 `CARGO_ZIGBUILD_ZIG_PATH` 指定时，自动使用 `cargo zigbuild`，不必设置构建器变量。
持续集成当前锁定 Zig 0.14.1 与
`cargo-zigbuild` 0.23.3；需要完全复现持续集成时，请安装这两个版本：

```bash
cargo install cargo-zigbuild --version 0.23.3 --locked
```

如果 Zig 不在 PATH，可只为 `cargo-zigbuild` 指定绝对路径：

```powershell
$env:CARGO_ZIGBUILD_ZIG_PATH = "C:\path\to\zig.exe"
```

若需要明确指定构建器，可设置：

```powershell
$env:PURE_REMOTE_HELPER_BUILDER = "zigbuild"
# 或强制使用系统 musl GCC
$env:PURE_REMOTE_HELPER_BUILDER = "cargo"
```

如果已有完整的目标专用交叉工具链，也可以直接指定每个目标的链接器（目标专用 Clang 或
musl GCC 均可）：

```powershell
$env:CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER = "C:\path\to\aarch64-linux-musl-clang.exe"
$env:CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "C:\path\to\x86_64-linux-musl-clang.exe"
$env:PURE_REMOTE_HELPER_BUILDER = "cargo"
```

这里的 Clang 必须已经包含对应目标的启动文件和 `musl` 系统库；普通桌面 Clang 不满足这个条件。

若已从持续集成或其他机器取得两个已校验的助手文件，也可以跳过本机交叉编译：

```powershell
$env:PURE_REMOTE_HELPER_PREBUILT_DIR = "C:\path\to\dist\remote-helper"
cargo xtask build-gui
```

不推荐把普通主机 `clang` 直接设置为 musl 链接器。Clang 只是编译器驱动，仍需要目标架构的
启动文件、`musl` 系统库和正确的目标参数；没有完整的目标系统根目录时会在链接阶段失败。Linux 桌面
图形界面的 Clang 预检与远程助手的 musl 交叉链接是两条独立链路。

#### Windows

- Visual Studio 2022 或 Build Tools，勾选“使用 C++ 的桌面开发”和 Windows 10/11 SDK；
- Windows 桌面支持：`cargo flutter config --enable-windows-desktop`；
- Rust 使用 `stable-x86_64-pc-windows-msvc` 工具链；
- CMake（Visual Studio 安装器可选装）应位于 PATH；
- 运行 `cargo xtask build-gui` 还需要前文的 Zig、`cargo-zigbuild` 和两个 musl Rust 目标。

检查环境：

```powershell
cargo flutter doctor -v
rustc -Vv
zig version
cargo zigbuild --version
rustup target list --installed
cargo xtask build-remote-helper --all-targets
cargo xtask build-gui
```

如果只需要验证 Rust 工作区，不构建桌面端，可跳过 Flutter、桌面编译器和远程助手准备，直接
运行 `cargo test --workspace`。如果 `build-gui` 报告缺少 musl 链接器，优先检查 `zig version`
与 `cargo zigbuild --version` 是否都能执行；两者都在 PATH 后，xtask 会自动切换到 Zig 构建器。

#### Linux

Flutter 的 Linux 桌面构建需要 Clang、CMake、Ninja、pkg-config、GTK 3 开发文件和 C++ 标准
库；无图形会话运行桌面集成测试还需要 Xvfb。Debian/Ubuntu 可一次安装：

```bash
sudo apt-get update
sudo apt-get install -y clang cmake ninja-build pkg-config build-essential libgtk-3-dev xvfb
cargo flutter config --enable-linux-desktop
cargo flutter doctor -v
```

xtask 会在构建前使用 PATH 中发现的真实工具链编译并链接最小 GTK/C++ 探针；不会写死编译器版本、
系统库路径，也不会注入 `LIBRARY_PATH` 或 `CPLUS_INCLUDE_PATH`。

未构建桌面内嵌资源、只运行 Rust 工作区测试或独立 Studio server 时，Linux 仍需要安装同一生产 worker：

```bash
cargo install --path code/pl-remote-helper --locked
```

确保安装目录位于 PATH。`exec` 不使用裸 shell 后备路径；桌面 xtask 构建会嵌入并保留 worker 可执行资源。

#### macOS

- 安装 Xcode，并执行 `xcode-select --install`；
- `cargo flutter config --enable-macos-desktop`；
- 接受 Xcode 许可：`sudo xcodebuild -license`；
- 运行 `cargo flutter doctor -v`，确保 CocoaPods（若项目依赖插件需要）可用。

#### Windows 正式发布（可选）

只有执行 `cargo xtask release-gui stage`/`finalize` 时才需要额外工具：

- Inno Setup 6（`ISCC.exe`，可通过 `ISCC_PATH` 指定路径）；
- `finalize` 需要 `minisign` 以及 `MINISIGN_SECRET_KEY_FILE`、`MINISIGN_PASSWORD`；
- 若启用安装包代码签名，需要 Windows SDK 的 `signtool.exe`，并设置 `WINDOWS_SIGNTOOL_PATH`、
  `WINDOWS_CERTIFICATE_PATH` 和 `WINDOWS_CERTIFICATE_PASSWORD`。

### 启动 Pure Studio 桌面应用

```powershell
# 当前 Windows/Linux/macOS 桌面目标（Flutter + flutter_rust_bridge v2）
cargo xtask run-gui
```

Flutter 端通过 `pl-studio-bridge` 调用同一个 `pl-studio-runtime`。每个打开的 Thread 只订阅自己的高频 Item/Turn/interaction 流；MCP/LSP health、配置和项目列表等低频事件走全局产品流。

首次启动后，在 Pure Studio 设置页面配置 LLM Provider。配置保存在：

```text
~/.pure/config.toml                 # 全局配置（provider、模型、角色）
~/.pure/studio/studio.sqlite        # Studio 项目、配置关联与产品事实
~/.pure/studio/sessions.sqlite      # 通用 Thread journal、不可变载荷与资源元数据
```

DeepSeek V4 的 Responses route 支持服务端原生联网搜索；Studio 默认启用
`[deepseek_web_search]`，当前 DeepSeek route 满足凭据、transport 和模型能力门控时优先使用，
否则回退到 `[web_search]` 配置的 OpenAI 搜索。模型保持 `tool_choice = auto`，并可同时使用普通函数、
MCP、LSP、文件和命令工具。

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
│   ├── pl-core/              # 通用 Thread、context/model/tool/storage
│   ├── pl-tool/              # 显式装配的工具与物理服务
│   ├── pl-remote-helper/     # 本地进程监督与 SSH 远端助手
│   ├── pl-studio-runtime/    # Studio 业务 runtime
│   ├── pl-studio-server/     # HTTP/OpenAPI/SSE server
│   └── pure-studio/          # Flutter 桌面应用与 FRB crate
├── design/                   # 架构设计文档及原型/视觉资产
├── .cargo/config.toml        # Cargo 配置
├── xtask/                    # pl-xtask 开发任务入口
└── AGENTS.md                 # 项目协作与工程规范
```

## 技术栈

| 层级 | 技术 |
|------|------|
| 桌面框架 | Flutter + flutter_rust_bridge v2 |
| 后端语言 | Rust（edition 2024） |
| 异步运行时 | tokio |
| 数据库 | SQLite via SeaORM（SQLx 后端） |
| 序列化 | serde + serde_json + toml |
| Flutter 状态管理 | Riverpod |
| Flutter 路由 | go_router |
| LLM 集成 | OpenAI 兼容 API、SSE 与 Responses WebSocket（async-openai 等适配） |
| LSP 客户端 | lsp-types + 自研 JSON-RPC framing（rust-analyzer 支持） |
| 流式解析 | async-openai stream |

## 核心概念

| 概念 | 说明 |
|------|------|
| **Thread** | 一个 agent 独占的对话、输入队列、会话工具任务与持久历史 |
| **Turn** | Thread 中一次由明确输入启动的模型执行；结束不丢弃已受理的会话任务 |
| **Item** | Studio 从已提交日志投影的消息、推理、工具、计划等内容单元 |
| **Tool** | 不透明 `Tool` 接口；Registration 转移实例给 Thread，每步冻结声明与执行租约 |
| **ToolTask** | 会话拥有的工具调用任务，以 `taskId` 查询、输入、取消与读取完整结果 |
| **LSP** | Language Server Protocol 客户端，支持代码智能查询（定义跳转、引用查找等） |
| **Agent** | 拥有独立 Thread 的执行身份，root 与 child 共用同一框架 |
| **Skill** | 带元数据与资源的可复用指令包，由运行时发现与激活 |
| **Studio** | 管理项目、配置与产品事件，由桌面和 HTTP 宿主共享 |
| **Provider** | LLM Provider 抽象（OpenAI、DeepSeek、智谱等） |
| **ThreadModeId** | root Thread 的 Mode 身份（例如 `mode.simple` / `mode.task`） |

### 内置工具

下列为主要工具类别；实际可见集合取决于能力配置、Profile、Thread Mode、Skill 与 MCP/LSP 状态：

| 分类 | 工具 |
|------|------|
| Shell | `exec`, `write_stdin`（内容搜索用 `rg`，文件搜索用 `rg --files`） |
| 文件读取 | `read_file`, `list_files`, `stat_path` |
| 文件写入 | `write_file`, `create_directory`, `delete_path`, `copy_path`, `move_path` |
| 补丁 | `apply_patch` |
| 代码智能 | `lsp_capabilities`, `lsp_query` |
| 子代理 | `spawn_agent`, `report_progress`, `send_message`, `interrupt_agent`, `list_agents`, `list_agent_profiles`, `read_agent_session`, `read_agent_submissions`, `close_agent` |
| 会话任务与事件 | `wait`, `sleep`, `list_tool_tasks`, `get_tool_task`, `cancel_tool_task` |
| 用户交互 | `request_user_input` |
| 技能 | `skills_list`, `skill_view`, `skill_manage` |
| MCP | 动态注册（`mcp__<server>__<tool>`） |
| 联网搜索 | DeepSeek/OpenAI Responses hosted search，或 OpenAI standalone search；由当前 route 能力自动仲裁 |

### 在其他应用中注册工具

宿主实现 `pl_core::model::ModelSession`，通过 `ThreadHandle` 创建 owner。工具实现
`pl_core::tool::opaque::Tool`，用 Registration 转移实例；完整 payload 和模型上下文分别保存，
工具自行解释格式与参数。core 不安装默认工具、不解析业务 JSON，也不代理 provider 配置。

Studio 用同一装配入口创建 root、child 和恢复 Thread，显式组合 pl-model/pl-tool 及产品工具。
动态目录只更新同一 Thread 注册表，已冻结调用持有原 executor 租约。扩展 CAS、交互和结束 Turn
使用显式注册授权，正文中的同名字段不能触发控制。

普通工具超过交付窗口后转为 Thread 后台任务，结果与消息一起提交，下一模型请求准入时消费。
历史使用当时保存的实际模型正文；资源关闭失败或保存失败保留 owner 和重试入口。
详见[工具边界](./design/28-tool-thread-boundary.md)和[任务交付](./design/26-session-tool-runtime.md)。

## 开发

### Rust 后端

```bash
# 与 CI 一致的 Rust 门禁
cargo fmt --all --check
cargo check -p pl-core --no-default-features --all-targets
cargo check -p pl-core --no-default-features --features sqlite --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

### Flutter 开发

Linux 原生构建需要可用的 C/C++ 工具链、CMake、Ninja、pkg-config 与 GTK 3 开发文件；无图形
会话的 desktop integration smoke 还需要 Xvfb。Debian/Ubuntu 可安装：

```bash
sudo apt-get install -y clang cmake ninja-build pkg-config build-essential libgtk-3-dev xvfb
```

xtask 会在 Flutter 构建前用当前 PATH 中的真实工具链编译、链接最小 GTK/C++ 探针。缺少命令、
C++ 标准库头文件或 GTK 链接库时会报告实际命令和原始输出，不依赖固定 Clang/GCC 版本，也不
注入机器专用的 include/library 路径。

远程无头 Web 交互验收还需要主版本匹配的 Chrome/Chromium 与 ChromeDriver，并需要显式启用
Flutter Web。Ubuntu 可使用 `chromium-browser chromium-chromedriver`，Debian 常用
`chromium chromium-driver`；其他发行版安装等价软件包即可：

```bash
cargo flutter config --enable-web
# 浏览器不在 PATH 时，在运行验证前设置 CHROME_EXECUTABLE=/absolute/path/to/chrome
```

```powershell
# 从仓库根目录执行一般 Flutter/Dart 命令，参数原样透传
cargo flutter analyze
cargo flutter test
cargo dart format lib

# 从仓库根目录解析依赖、静态分析并运行非视觉测试
cargo xtask verify-gui

# 在当前 Windows/Linux 桌面目标运行 integration smoke；Linux headless 自动使用 Xvfb
cargo xtask verify-gui --integration

# 在临时本地端口启动 ChromeDriver，以 Flutter Web demo 跑同一套无头交互 smoke
cargo xtask verify-gui --web-integration

# 显式使用已安装配置、真实 provider/model 与 API credential 验收统一工作流
cargo xtask verify-workflow --live --headless
cargo xtask verify-workflow --live --gui

# 从仓库根目录运行 GUI
cargo xtask run-gui

# 从仓库根目录构建当前 OS 的 release 产物
cargo xtask build-gui
```

Markdown/timeline 视觉检查可以使用本地 demo 数据启动，不连接 runtime：

```powershell
cargo xtask run-gui --demo
```

`--web-integration` 只验证纯 Dart demo 的布局、路由、交互与状态投影，不替代桌面 Rust bridge
或真实 server/model 验收。xtask 自动发现浏览器和 driver、校验主版本、处理 wrapper/sandbox
封装、选择空闲端口并回收进程树；失败时原始驱动日志保存在
`code/pure-studio/build/web-integration-artifacts`。Playwright 可作为额外截图或可访问性观察层，
但 canonical 交互断言仍使用 Flutter integration test 的稳定 `ValueKey`。

`verify-workflow --live` 会产生真实模型调用和费用，不进入默认 CI，也不会回退到 scripted
provider。GUI 路径启动真实 native Studio，在 `planning` 中先通过 `request_user_input` 补齐事实，再由
固定 Plan 状态机的 `plan_submit` 完成修订与批准；批准后的完整 Plan 作为 GUI 隐藏的用户消息进入同一
Thread。完成后 durable shutdown，再以同一隔离
Studio home 恢复相同 Thread 与 workflow run。脱敏 wire、workflow snapshot、命令输出、截图、
render tree 和 Driver 日志保存在 `target/workflow-live-artifacts/`。

本仓库要求 Flutter 端使用 `flutter_rust_bridge` v2.12.x；本机 codegen 版本应与 Dart/Rust 依赖保持同一小版本。

## 设计文档

项目完整的架构决策和设计说明收录在 [`design/`](./design/) 目录：

| 文档 | 内容 |
|------|------|
| [01-overview.md](./design/01-overview.md) | 系统总览与定位 |
| [02-crates.md](./design/02-crates.md) | Crate 设计与端口-适配器架构 |
| [03-pipeline.md](./design/03-pipeline.md) | Thread / Turn / Item 流程 |
| [04-security.md](./design/04-security.md) | 安全与权限模型 |
| [05-extension.md](./design/05-extension.md) | 扩展机制 |
| [06-phases.md](./design/06-phases.md) | 实施阶段 |
| [07-model.md](./design/07-model.md) | 模型与 Provider 设计 |
| [08-streaming.md](./design/08-streaming.md) | Thread 实时流 |
| [09-conventions.md](./design/09-conventions.md) | 编码约定 |
| [10-config.md](./design/10-config.md) | 配置系统 |
| [11-studio-ui.md](./design/11-studio-ui.md) | Studio UI 设计 |
| [12-unified-workflow-implementation-spec.md](./design/12-unified-workflow-implementation-spec.md) | 统一模式与工作流实施规约 |
| [13-skills.md](./design/13-skills.md) | 技能系统设计 |
| [13-tool-calling-runtime.md](./design/13-tool-calling-runtime.md) | 工具调用运行时 |
| [14-lsp-runtime.md](./design/14-lsp-runtime.md) | LSP 运行时 |
| [15-agent-profiles-and-collaboration.md](./design/15-agent-profiles-and-collaboration.md) | Agent Profile 与统一协作 |
| [16-task-orchestration.md](./design/16-task-orchestration.md) | Thread Mode 与预注册工作流 |
| [17-agent-runtime-host.md](./design/17-agent-runtime-host.md) | Agent runtime 与宿主边界 |
| [18-studio-release-update.md](./design/18-studio-release-update.md) | Studio 发布与更新 |
| [19-studio-storage-and-diagnostics.md](./design/19-studio-storage-and-diagnostics.md) | Studio 存储与诊断 |
| [20-studio-state-runtime.md](./design/20-studio-state-runtime.md) | Studio 状态查询与领域生命周期 |
| [21-session-activation-and-persistence.md](./design/21-session-activation-and-persistence.md) | 会话激活、唯一热状态与异步持久化 |
| [22-ssh-remote-development.md](./design/22-ssh-remote-development.md) | SSH 远程开发与宿主能力 |
| [23-thread-mode.md](./design/23-thread-mode.md) | Thread Mode 注册、图生命周期与工具合同 |
| [24-agent-session-plan.md](./design/24-agent-session-plan.md) | Thread 内的产品 Plan 状态机与通用隐藏 continuation |
| [25-session-entry-storage.md](./design/25-session-entry-storage.md) | 统一会话条目与独立存储 |
| [26-session-tool-runtime.md](./design/26-session-tool-runtime.md) | 会话工具任务与统一唤醒 |
| [27-core-boundaries-and-replay.md](./design/27-core-boundaries-and-replay.md) | 极简核心、动态载荷与重放合同 |
| [28-tool-thread-boundary.md](./design/28-tool-thread-boundary.md) | 工具实例、参数、结果与 Thread 边界 |

## 项目规范

详细的编码约定和协作规则见 [`AGENTS.md`](./AGENTS.md)。

## License

Apache-2.0
