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
- 所有 crate 在 `code/` 下：`pl-protocol`、`pl-trace`、`pl-model`、`pl-lsp`、`pl-core`、`pure-studio/rust`（pl-studio-bridge）。
- 跨 crate 依赖方向：`pl-protocol` ← `pl-trace` ← `pl-model` ← `pl-core` ← `pl-studio-bridge`。
- 模块大小目标 ≤500 行（不含测试），超 800 行强制拆分。
- `frb_generated.rs` 为自动生成文件，不参与审查。

## 扫描命令参考

以下 rg 命令可以直接用于发现对应问题。注意排除 `*frb_generated*` 和测试文件。

### 文件行数（生产代码行数）
```
# 列出所有超限文件，并排除嵌入式测试模块
Get-ChildItem -Recurse "*.rs" -Exclude "*frb_generated*" | ForEach-Object {
  $total = (Get-Content $_.FullName | Measure-Object -Line).Lines
  $testStart = (Get-Content $_.FullName | Select-String -Pattern '#\[cfg\(test\)\]' | Select-Object -First 1).LineNumber
  $prod = if ($testStart) { $testStart - 1 } else { $total }
  if ($prod -gt 500) { "{0,5}  {1}" -f $prod, $_.FullName }
} | Sort-Object -Descending
```

> 注意：单独文件（如 `tests.rs`）和 `tests/` 目录下的文件不在行数统计范围内。

### 大结构体字段数扫描
```
# 扫描生产代码中字段数 > 8 的结构体，排除 frb_generated.rs 和 build 目录
Get-ChildItem -Recurse -Filter "*.rs" code/ | Where-Object {
  $_.FullName -notmatch "\\target\\" -and
  $_.FullName -notmatch "frb_generated" -and
  $_.FullName -notmatch "\\build\\"
} | ForEach-Object {
  $content = Get-Content $_.FullName -Raw
  if ($content -match '#\[cfg\(test\)\]') { $content = ($content -split '#\[cfg\(test\)\]')[0] }
  $matches = [regex]::Matches($content, '(?ms)pub struct (\w+) \{(.*?)\}')
  foreach ($m in $matches) {
    $name = $m.Groups[1].Value
    $body = $m.Groups[2].Value
    $fields = ([regex]::Matches($body, '^\s+pub \w+', 'Multiline')).Count
    if ($fields -gt 8) {
      "{0,3} fields {1}  {2}" -f $fields, $name, $_.FullName.Replace((Get-Location).Path + "\", "")
    }
  }
} | Sort-Object -Descending
```

> 判断大结构体是否需要拆分时，区分两类：**DTO/序列化类型**（如 pl-protocol 和 bridge 中的事件类型）字段数虽多但天然需要统一序列化格式，拆分反增复杂度；**运行时状态/配置类型**（如超过 10 字段的 `TurnResult`、`ToolContext`）可以考虑按职责 grouping 重构。

### 异步 Trait 违规
```
rg '#\[async_trait\]' code/ --glob '*.rs'  -n
rg '#\[allow\(async_fn_in_trait\)\]' code/ --glob '*.rs'  -n
```

### `bool` 参数和 `Option<bool>`
```
rg 'fn .*\b\w+: bool\b' code/ --glob '*.rs' --glob '!*frb_generated*' -n
rg ': Option<bool>' code/ --glob '*.rs' --glob '!*frb_generated*' -n
```

### `_ => {}` 静默忽略
```
rg '_ => \{\}' code/ --glob '*.rs' -n
```

### Serde `rename_all` 合规检查
```
# 找出 derive Serialize/Deserialize 且没有 rename_all 的类型
rg '#\[derive\(.*[Ss]erialize.*\)\]' code/ --glob '*.rs' --glob '!*frb_generated*' -A3 -n 2>$null | rg -v 'rename_all' | rg 'derive.*[Ss]erialize'
```
> 注意：`pl-trace` 内部 trace 类型、`pl-core/src/mcp/wire.rs` 的 JSON-RPC 类型、`pl-lsp/src/server_definition.rs` 的内部配置类型可能有正当理由不使用 camelCase。逐个判断后记录例外。

### `format!` 位置参数（非内联变量）
```
# 生产代码中查找，排除测试文件
rg 'format!\("[^"]*\{\}' code/ --glob '*.rs' --glob '!*tests*' --glob '!*test*' --glob '!*frb_generated*' -n
```

### 生产代码 `unwrap()` / `expect()`
```
# 排除 *tests* / *test* / *live* 是快速过滤，但嵌入式测试模块仍会命中，需二次确认
rg '\.unwrap\(\)|\.expect\(' code/ --glob '*.rs' --glob '!*tests*' --glob '!*test*' --glob '!*frb_generated*' -n
```

### `unwrap_or_default()` 掩盖错误
```
rg 'unwrap_or_default\(\)' code/ --glob '*.rs' -n
```

### 跨 crate 分组统计
将扫描结果按 crate 分组比直接看文件列表更易把握分布：
```powershell
$hits = rg '<PATTERN>' code/ --glob '*.rs' -n 2>$null
$hits | ForEach-Object { ($_ -split '\\')[1] } | Group-Object | Sort-Object Count -Descending
```
此法适用于 `format!` 位置参数、`unwrap()` 等分布较广的问题。

### TODO / FIXME / HACK / XXX
```
rg 'TODO|FIXME|HACK|XXX' code/ --glob '*.rs'  -n
```

## 审查清单

逐 crate/crate 检查以下项目：

### 1. 模块大小
- 列出所有超 500 行的模块（建议拆分），超 800 行的（必须拆分）。
- 当前主要关注：`pl-patch/src/lib.rs`、`pl-core/src/tool/git.rs`、`pl-model/src/protocol/openai/sse/mod.rs`、`pl-protocol/src/studio_event.rs`、`pl-core/src/core/turn_loop/mod.rs`、`pl-model/src/stream/mod.rs`。
- 已拆分完毕的旧大文件不再列入扫描范围（如 `studio/store.rs`、`studio/runtime.rs`、`api/studio.rs`、`core/mod.rs` 已在过往审查中拆分）。

### 2. 异步 Trait
- 搜索 `#[async_trait]` 和 `#[allow(async_fn_in_trait)]` — 全项目禁止使用。
- 所有异步 trait 方法必须用原生 RPITIT + 显式 `Send` bound。

### 3. 参数设计
- 搜索 `fn.*: bool)` 和 `Option<bool>` 模式，检查调用点是否能理解参数含义。
- 语义模糊的应改为枚举：`WorkspaceEscapePolicy { AllowEscape, RestrictToWorkspace }`。
- 已有代码库示例：`pl-model/src/visible_text.rs` 将 `drain_pending(finish: bool)` 改为 `DrainMode::{Partial, Final}` 枚举，调用点从 `true`/`false` 变为 `DrainMode::Final`/`DrainMode::Partial`，语义自明。
- 双 bool 参数（如 `ProcessManagerState::new(stdout_open: bool, stderr_open: bool)`）可考虑 bitflags 或 options struct，但若只有少量调用点可暂缓。

### 4. Match 穷尽匹配
- 搜索 `_ => {}` 模式，确认不是在枚举 match 中静默忽略变体。
- 区分两种情况：
  - **真正静默忽略**：`_ => {}` 后直接结束 match 块，应改为记录警告日志。
  - **有意 fall-through**：`_ => {}` 后继续执行 match 之后的代码（如 SSE 解码器中未命中特殊处理的事件让 legacy 处理器兜底），可添加 `tracing::trace!` 日志辅助调试，而非改为警告。
- 尤其注意 SSE 解析器中的字符串 match，如果用 `_ => {}` 应至少记录 trace 日志。

### 5. Trait 文档
- 所有新增 trait 必须有大段文档注释，说明角色和实现/使用方式。

### 6. API 边界（Serde 命名）
- 所有 `#[derive(Serialize, Deserialize)]` 类型必须标注 `#[serde(rename_all = "camelCase")]`。
- 例外：`WriteMode` 用 `lowercase`（"create"/"overwrite"/"append"），已确认合理。
- Rust 字段保持 `snake_case`。
- 时间戳用 `i64`，命名为 `*_at`。ID 用 `String`。

### 7. 格式化偏好
- 搜索 `format!("{}",` 模式，必须使用内联变量 `format!("{name}")`。
- Rust format! 只支持标识符内联（`format!("{var}")`），不支持 `{expr.field}` 或 `{fn()}` 语法。字段访问需要引入临时变量或使用命名参数。
- 闭包体内的 `format!` 如果参数是方法调用（如 `.display()`、`.len()`），引入临时变量会显著降低可读性，可以保留位置参数。

### 8. `unwrap()` / `expect()` 在非测试代码中
- 生产代码（非 `#[cfg(test)]`）禁止 `unwrap()` 或 `expect()`。
- 发现后应替换为 `?` 操作符或更安全的错误处理。
- Mutex 中毒场景可复用的恢复模式：
  ```rust
  let mut locks = self.locks.lock().unwrap_or_else(|poisoned| {
      tracing::warn!("lock was poisoned, recovering");
      poisoned.into_inner()
  });
  ```

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
| 4 | `pure-studio/rust/` (bridge) | 模块大小、FRB 错误转换、format! 内联 |

父 agent 合取各子报告，交叉验证，输出汇总表和修复计划。

## 执行器子代理分工

修复阶段可使用 `spawn_agent profileId: "executor"` 按 crate 并行实施机械化修改。

典型分工：

| Agent | Scope | 包含修改 |
|-------|-------|----------|
| pl-lsp | `code/pl-lsp/src/` | format! 内联、serde 补全、unwrap_or_default 安全化 |
| pl-model | `code/pl-model/src/` | format! 内联、SSE 日志、bool→enum 重构 |
| pl-core | `code/pl-core/src/` | format! 内联、Mutex expect 恢复、serde 补全 |

每个执行器 agent 的任务应包含：
- 明确的文件列表和行号（来自审查扫描结果）
- 精确的转换规则（什么模式改成什么模式）
- 该 crate 专属的额外修复项
- 不修改测试文件、自动生成文件的约束
- 完成后需 `cargo fmt` 的指令

父 agent 在所有执行器完成后：
1. 运行 `cargo fmt` 统一定格式
2. 运行 `cargo clippy --workspace --all-targets -- -D warnings` 验证
3. 运行 `cargo test --workspace` 确认回归
4. 汇总各执行器交付结果

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

#### 拆分前：子代理预分析
拆分不能靠猜测，需要先理解大文件的内部结构。建议在动手前使用 explorer subagent 分析目标文件：
- 列出所有 `pub struct`、`pub enum`、`pub fn`、`impl`、`trait` 及其大致行数范围
- 识别属于不同功能关注点的类型和函数
- 判断是否有字段数 > 10 的大结构体，区分 DTO 与运行时状态
- 定位 `#[cfg(test)]` 边界，确认生产代码实际行数

子代理输出示例（结构化列表）：
```
| 定义                | 角色               | 行数范围 |
|---------------------|-------------------|---------|
| ExecutionRequest    | 通用命令执行请求   | 29-39   |
| LocalExecutionBackend | 本地进程执行后端 | 89-107  |
| GitCredential       | 凭证封装          | 120-135 |
```

父 agent 基于分析结果制定拆分方案：确定新模块列表、每个模块包含的 API、重导出策略，然后才进入代码修改。

#### 拆分后验证
每个文件拆分完成后：
1. 运行 `cargo fmt`
2. 运行 `cargo clippy -p <crate> -- -D warnings`
3. 运行 `cargo test -p <crate>`
4. 检查下游 crate 编译：`cargo check -p <下游crate>`

#### 优先拆分顺序（当前状态）
1. `pl-patch/src/lib.rs`（915 行生产代码）— 整个 crate 只一个文件，按功能拆为 error/parse/apply/match_util
2. `pl-core/src/tool/git.rs`（766 行）— 多职责混杂，拆为 execution/credential/policy/types/helpers 子模块
3. `pl-model/src/protocol/openai/sse/mod.rs`（721 行）— 拆为 types/decoder 子模块
4. `pl-protocol/src/studio_event.rs`（639 行）— 按事件域拆为 message/part/agent/session/health 子模块
5. 其余超 500 行的文件和模块视内部结构和变更频率决定是否拆分

## 验证

完成修改后执行：
```powershell
cargo fmt
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

结果必须零 warning（clippy `-D warnings`）且所有测试通过。
