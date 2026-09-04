---
name: sync-readme
description: Use when synchronizing the root README.md with the current codebase, manifests, design index, runtime entrypoints, and verification rules.
category: guides
platforms: [windows, linux, macos]
---

# 同步根 README

README 的架构图、crate 表、项目结构、技术栈、核心概念、工具目录和设计索引必须来自当前仓库
事实。不要在本技能中维护容易腐化的固定成员清单、工具数量或模块列表。

## 同步流程

### 1. 工作区与产品入口

- 从根 `Cargo.toml` 的 `[workspace].members` 取得完整 Cargo 成员。
- 从每个成员自己的 `Cargo.toml` 核验包名和直接依赖；目录名与包名不同时明确标注，例如
  `code/pure-studio/rust` 的包名为 `pl-studio-bridge`。
- 单独核验非 Cargo 成员的 Flutter 客户端 `code/pure-studio/`、系统技能资源和 xtask 入口。
- README 架构图、crate 表、依赖规则和项目结构树必须使用同一批事实。

### 2. 设计文档

- 用实际 `design/*.md` 文件集合核对 README 设计索引，不固定文档数量。
- 排除 `design/assets/` 等非文档资源。
- 核对链接目标、编号、标题和描述；新增、删除或重命名文档时同步所有引用。

### 3. 工具运行时

- 从 `TurnEngine::install_default_tools`、`BuiltinToolInstaller`、`ToolCapabilityConfig` 及产品运行时的
  动态安装入口核对工具来源。
- 区分静态内置工具、按能力安装的工具族、MCP/LSP、协作工具和 hosted 工具，不把它们误写成
  单一固定注册表。
- 如 README 需要数量，必须从当前安装条件逐项计算并注明条件；优先展示稳定类别和名称，不保存
  无法自动核验的总数。

### 4. 项目结构与核心概念

- 使用 `rg --files`、目录枚举和 crate 根导出核对实际模块，不复制旧目录快照。
- 从 `design/01-overview.md`、`design/02-crates.md` 及当前公共类型核对 Thread、Turn、Item、Agent、
  Tool、Skill、Studio、Provider、MCP、LSP 与 Thread Mode 等概念。
- 只保留已经实现且对 README 受众重要的顶层概念；实验名词和已删除类型不得继续出现。

### 5. 技术栈与命令

- 从 workspace manifests、Flutter `pubspec.yaml`、xtask 与 CI 配置核对依赖和版本。
- 命令必须遵循根 `AGENTS.md`：Flutter/Dart 通过仓库包装入口，GUI 通过 xtask，不提供绕过路径。
- 架构或运行行为声明必须能回指代码、设计或清单；无法核实的内容应删除或标注限制。

## 验证

README 修改本身至少执行链接和路径核验以及 `git diff --check`。若同时改变代码或配置，则按
`AGENTS.md` 执行对应门禁：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo xtask verify-gui
git diff --check
```

`cargo check --workspace` 只能证明代码仍可编译，不能证明 README 中的数量、链接或架构事实正确。
