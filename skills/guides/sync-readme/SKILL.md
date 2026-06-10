---
name: sync-readme
description: Use when synchronizing the root README.md with the actual codebase state. Covers systematic diff against project structure, crate list, tool registry, design documents, and architecture diagram.
category: guides
platforms: [windows, linux, macos]
---

# Sync Root README.md with Codebase

Pure-Lang 的 `README.md` 包含架构图、Crate 表、项目结构树、技术栈、核心概念、内置工具清单和设计文档索引。当项目结构或功能发生变更后，需要将 README 与实际代码库同步。

## 同步清单

### 1. Workspace Crate — 与 `Cargo.toml` 交叉验证

读取 `Cargo.toml` 的 `[workspace].members`，与 README 中以下两个位置的 crate 列表对比：

- **架构图**（`## 架构` 下方的 `Workspace Crate` 表）
- **项目结构树**（`### 项目结构` 下的 `code/` 目录）

当前 members：`pl-protocol`、`pl-model`、`pl-lsp`、`pl-core`、`pure-studio/src-tauri`

注意：`pure-studio/src-tauri` 在 README 中写作 `pure-studio`（反映目录而非 Cargo package 名）。

### 2. 工具数量 — 与 `register_default_tools()` 对比

在 `code/pl-core/src/core/mod.rs` 中找到 `register_default_tools()` 函数，统计所有 `self.register_tool(...)` 调用数量。与 README 中 `### 内置工具` 表格的合计数量对比。

当前内置工具：23 个（不含 MCP 动态工具）。

### 3. 设计文档 — 与 `design/` 目录对比

列出 `design/` 目录下所有 `.md` 文件，与 README 中 `## 设计文档` 表格对比。

- 检查文件数量（README 描述中的份数是否准确）
- 检查每个文件路径和描述是否正确
- `design/` 目录下的 `assets/` 子目录（图片等）不列入表格

### 4. 项目结构树 — 与实际目录对比

`### 项目结构` 中的目录树需要与 `code/` 下各 crate 的实际 `src/` 子目录对比：

- `pl-core/src/`：检查 `src/` 下的一级目录（agent、config、core、domain、infrastructure、interfaces、mcp、skill、studio、tool 等）
- `pl-core/src/tool/`：检查工具子模块（command、file、multi_agent 及单文件工具）
- 新增的 crate（如 `pl-lsp`）需要补充
- 新增的模块（如 `src/mcp/`、`src/tool/command/`）需要补充

### 5. 架构图 — 与 crate 依赖关系对比

`## 架构` 中的 ASCII 图应反映顶层的 crate 依赖关系：

- `pure-studio → pl-core`（核心依赖）
- `pl-core → pl-model`（provider 抽象）
- `pl-core → pl-lsp`（LSP 客户端）
- `pl-core → pl-protocol`（公共协议）

同时在 `### 依赖规则` 中补充两条依赖链：

```
pl-protocol  ←  pl-model  ←  pl-core  ←  pure-studio
                pl-lsp     ←  pl-core
```

### 6. 核心概念 — 检查新概念

在 `## 核心概念` 表格中补充项目中新增的顶层概念。当前概念完整列表：

- Turn, Session, Tool, Agent, Skill, Studio, Provider, MCP, LSP, CompileMode

如需补全新概念，保持表格风格一致：`**概念名** | 一句话说明`

### 7. 技术栈 — 检查新增依赖

在 `## 技术栈` 表格中补充 README 中缺失的技术依赖：

- LSP 客户端：`lsp-types + 自研 JSON-RPC framing（rust-analyzer 支持）`

### 8. 验证

写入后通过 `cargo check --workspace` 确认 README 修改未破坏任何内容（严格来说这不是必要的，但作为好习惯）。

## 典型触发场景

- 新增或删除 crate
- 新增内置 tool 或 tool 分类重组
- 新增设计文档
- 新增顶层核心概念（如 MCP、LSP）
- 新增技术栈依赖
