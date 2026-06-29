---
name: code-review
description: Use when performing code quality review across Pure-Lang crate workspace. Covers review checklist, subagent partitioning, common finding patterns, and staged remediation approach.
category: guides
platforms: ["windows", "linux", "macos"]
---

# Code Review Guide for Pure-Lang

对整个 Pure-Lang workspace 做代码质量审查时使用。核心思路：用子代理并行审查各 crate，系统化对照 AGENTS.md 和 design 约定，最后按阶段修复。

## 前置知识

- AGENTS.md 和 `design/09-conventions.md` 是审查的约定依据。
- 所有 crate 在 `code/` 下：`pl-protocol`、`pl-trace`、`pl-model`、`pl-lsp`、`pl-core`、`pure-studio-flutter/rust`（pl-studio-bridge）。
- 跨 crate 依赖方向：`pl-protocol` ← `pl-trace` ← `pl-model` ← `pl-core` ← `pl-studio-bridge`。
- 模块大小目标 ≤500 行（不含测试），超 800 行强制拆分。

## 审查清单

逐 crate/crate 检查以下项目：

### 1. 模块大小
- 列出所有超 500 行的模块（建议拆分），超 800 行的（必须拆分）。
- 尤其关注：`studio/store.rs`、`studio/runtime.rs`、`core/mod.rs`、`studio/event_runtime.rs`、`studio.rs`、`api/studio.rs`（bridge）。

### 2. 异步 Trait
- 搜索 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]` — 全项目禁止使用。
- 所有异步 trait 方法必须用原生 RPITIT + 显式 `Send` bound。

### 3. 参数设计
- 搜索 `fn.*: bool)` 和 `Option<bool>` 模式，检查调用点是否能理解参数含义。
- 语义模糊的应改为枚举：`WorkspaceEscapePolicy { AllowEscape, RestrictToWorkspace }`。

### 4. Match 穷尽匹配
- 搜索 `_ => {}` 模式，确认不是在枚举 match 中静默忽略变体。
- 尤其注意 SSE 解析器中的字符串 match，如果用 `_ => {}` 应记录警告日志而非静默忽略。

### 5. Trait 文档
- 所有新增 trait 必须有大段文档注释，说明角色和实现/使用方式。

### 6. API 边界（Serde 命名）
- 所有 `#[derive(Serialize, Deserialize)]` 类型必须标注 `#[serde(rename_all = "camelCase")]`。
- 例外：`WriteMode` 用 `lowercase`（"create"/"overwrite"/"append"），已确认合理。
- Rust 字段保持 `snake_case`。
- 时间戳用 `i64`，命名为 `*_at`。ID 用 `String`。

### 7. 格式化偏好
- 搜索 `format!("{}",` 模式，必须使用内联变量 `format!("{name}")`。
- 项目 edition 2024，支持 `format!("{expr.field}")` 和 `format!("{fn()}")`。

### 8. `unwrap()` / `expect()` 在非测试代码中
- 生产代码（非 `#[cfg(test)]`）禁止 `unwrap()` 或 `expect()`。
- 发现后应替换为 `?` 操作符或更安全的错误处理。

### 9. TODO / FIXME / HACK / XXX
- 搜索并列出所有出现，评估是否需要立即处理或归档 issue。

### 10. 错误处理
- `unwrap_or_default()` 是否可能掩盖错误？尤其是在 JSON 反序列化和数据库读取路径上。
- 是否混用 `anyhow::Result` 和 `pl_protocol::Result`？Studio 模块已知使用 `anyhow`，但应与全项目统一策略。

### 11. 辅助方法
- 不为只调用一次的逻辑创建辅助函数。避免过度抽象。

### 12. Crate 边界
- 确保不违反依赖方向：`pl-model` 不得依赖 `pl-core`。
- `pl-protocol` 不依赖任何内部 crate。
- `pl-lsp` 不依赖 `pl-core`。

## 子代理分区策略

对整个 workspace 审查时，按 crate 或 crate 分组分配 explorer agent：

| Agent | Scope | 重点关注 |
|-------|-------|----------|
| 1 | `pl-core/src/` | 模块大小、Studio 模块 anyhow 混用、错误处理、安全 |
| 2 | `pl-model/src/` | SSE 健壮性、serde rename_all、unwrap/expect |
| 3 | `pl-lsp/` + `pl-protocol/` + `pl-trace/` | 模块大小、format! 内联、死参数、LSP 资源管理 |
| 4 | `pure-studio-flutter/rust/` (bridge) | 模块大小、FRB 错误转换、format! 内联 |

父 agent 合取各子报告，交叉验证，输出汇总表和修复计划。

## 常见发现模式

根据此前全量审查经验，以下问题最常见：

### 🔴 严重（频繁出现）
- 模块规模超标（占发现总数的 60%+）
- SSE 解码器中 `_ => {}` 静默忽略事件类型
- 语义模糊的 `bool` 参数

### 🟡 中等（偶发）
- 序列化类型缺少 `rename_all = "camelCase"`
- 死参数 / 未使用变量通过 `let _ =` 抑制
- if-else 链替代枚举穷尽 match
- `format!` 使用位置占位符而非内联变量
- `unwrap_or_default()` 静默掩盖反序列化错误

### 🔵 建议
- `debug_assert!` 缺少 `# Panics` 文档注释
- 辅助函数只调用一次
- 大量 import 集中在 `core/mod.rs` 顶部

## 阶段性修复策略

对检出的大量问题，按阶段推进：

### 第一阶段：快速修复（低风险）
- `_ => {}` 改为记录警告日志
- `bool` 参数改为枚举
- 移除死参数
- if-else 改为穷尽 match
- `format!` 内联化
- 生产代码中的 `expect()` 改为安全处理
- `unwrap_or_default()` 改为错误传播

### 第二阶段：API 一致性（中风险）
- 补全缺失的 `rename_all = "camelCase"`
- 统一错误类型（如 Studio 模块的 anyhow → pl_protocol::Result）

### 第三阶段：模块拆分（高风险，逐个验证）
每次拆分一个文件，确保 `cargo test -p <crate>` 全通过后继续下一个。

优先拆分顺序：
1. `studio/store.rs`（3039 行）— 按领域拆出 migration、event-persistence、setting
2. `studio/runtime.rs`（2363 行）— 按职责拆出 session-lifecycle、prompt-orchestration
3. `api/studio.rs`（2203 行）— 拆为 types、handlers、convert、runtime
4. `core/mod.rs`（1937 行）— 按编译阶段拆分 turn lifecycle、instruction assembly
5. 其余超 800 行文件

## 验证

完成修改后执行：
```powershell
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

结果必须零 warning（clippy `-D warnings`）且所有测试通过。
