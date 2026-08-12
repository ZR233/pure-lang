---
name: add-tool
description: Use when adding a new built-in tool to Pure-Lang. Covers Tool trait implementation, schema definition, registration chain, permission policy, event emission, callback patterns, and cross-crate export.
category: guides
platforms: ["windows", "linux", "macos"]
---

# Add a New Built-in Tool

在 Pure-Lang 中添加新内置工具时，工具系统位于 `pl-core` 内部（`code/pl-core/src/tool/`），目前没有独立的 `pl-tool` crate。共享工具由 `ToolSetBuilder` 按 capability、backend 和可见性统一组装，再注册到 `TurnEngine` 的 `ToolRegistry`；产品专用工具由宿主显式注入，不另建兼容注册链。

## 前置确认

0. **搜索现有实现**：在提案前，先搜索代码库确认同名 tool 是否已存在。在 `pl-core/src/tool/` 目录和 `register_default_tools()` 中搜索工具名。避免为本已实现的工具重复提案。

1. **确定工具名**：snake_case，全局唯一（`ToolRegistry::register()` 同名断言防止重复）。
2. **确定 typed 输入**：静态 function tool 使用 `Deserialize + JsonSchema` 的 Rust struct/enum；
   `serde_json::Value` 只保留在 provider、MCP 和其他运行时动态边界。
3. **确定并行与缓存语义**：`supports_parallel_tool_calls()` 默认 `false`；只读工具按需声明 `ToolCachePolicy`，写入或进程工具必须正确声明 cache invalidation。
4. **确定工具类型**：绝大多数工具是 `ToolSchema::Function`（JSON Schema 参数）；`apply_patch` 是唯一的 `ToolSchema::Custom`（Lark grammar）。
5. **确定 effect 与权限边界**：为工具声明 `ToolEffect`；涉及路径或 cwd 时同步更新统一的路径提取和风险说明；写工具是否需要 `workspace_write_lock`？
6. **确定是否需要交互**：复用 `TurnOptions::interaction_callback`、`InteractionRequest` 和 `InteractionResolution`，不要新增工具私有 callback 或第二套审批通道。
7. **确定是否发运行时事件**：工具结果内的结束回合、artifact 等语义使用 `ToolRuntimeEvent`；只有跨运行时的稳定事实才扩展共享协议。

## 修改清单

### 1. `pl-core/src/tool/<new_tool>.rs` — Tool 实现

创建新文件，实现 `Tool` trait：

```rust
use pl_protocol::PureError;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::tool::*;

#[derive(Debug, Default)]
pub struct MyNewTool;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MyNewToolInput {
    /// What to search for.
    query: String,
    /// Optional predefined choices.
    options: Option<Vec<String>>,
}

impl Tool for MyNewTool {
    fn name(&self) -> &str {
        "my_new_tool"
    }

    fn description(&self) -> &str {
        "Description shown to LLM explaining when to use this tool."
    }

    fn input_schema(&self) -> serde_json::Value {
        FunctionToolDefinition::<MyNewToolInput>::new(self.name(), self.description())
            .input_schema()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        Box::pin(async move {
            let args = deserialize_tool_input::<MyNewToolInput>(self.name(), input.arguments)?;
            let result = run_query(
                &context.workspace_root,
                &args.query,
                args.options.as_deref().unwrap_or_default(),
            )
            .await?;

            ToolOutput::json(serde_json::json!({
                "result": result,
            }))
        })
    }
}
```

产品专用静态工具不需要实现 `Tool` 时，直接使用 typed builder：

```rust
let tool = FunctionToolDefinition::<MyNewToolInput>::new(
    "my_new_tool",
    "Description shown to LLM.",
)
.registered(|input, context| async move {
    run_product_tool(input, context).await
})
.with_effect(ToolEffect::Read)
.with_parallel_tool_calls();
```

同一工具族中完全同义的字段组应抽为命名 struct，并在顶层用 `#[serde(flatten)]` 组合；不要复制
字段，也不要建立包含大量无关 `Option` 的万能参数类型。flatten 字段名不得重叠，未知顶层字段由
`deserialize_tool_input` 统一拒绝。

### 2. `pl-core/src/tool/mod.rs` — 模块声明 + 导出

- 添加 `mod <new_tool>;`
- 在明确的工具族聚合边界添加 `pub use <new_tool>::*;`，不要增加只改名的 re-export 薄层。
- glob 会导出该模块中所有 `pub` item；输入类型和实现细节默认保持私有或 `pub(crate)`，只有稳定
  API 才声明为 `pub`。跨领域且需要筛选的边界继续显式导出。

### 3. `pl-core/src/core/tool_set.rs` — 进入唯一共享注册链

- 在 `ToolSetBuilder::register()` 中按现有 capability 注册工具。
- 在 `shared_tool_schemas()` 对应的 schema 集合中同步加入模型可见 schema。
- 如果工具依赖可选 backend，为 `ToolCapabilityConfig` / `SharedToolSchemaOptions` 添加一个明确 capability；不要在 `TurnEngine`、Studio 和子代理入口各维护一份注册列表。
- `TurnEngine::register_tool()` 只用于产品专用工具、测试或宿主显式注入，不作为共享默认工具的第二条注册路径。

### 4. `pl-core/src/turn/execution.rs` — 声明工具 effect

在 `ToolEffect::for_builtin_name()` 中加入新内置工具，或在实现中覆盖 `effect()`：

```rust
fn effect(&self) -> Option<ToolEffect> {
    Some(ToolEffect::Read)
}
```

角色和阶段权限由 `AgentExecutionPolicy.allowed_effects` 与 `ToolVisibilitySet` 统一限制，不再维护 Plan mode 工具名白名单。

### 5. `pl-core/src/core/permission.rs` — 路径和审批边界

工具本身不决定是否审批。若参数包含路径或 cwd：

- 在 `requested_paths_for_tool()` 中提取所有可能越过 workspace 的路径。
- 在 `permission_risk_summary()` 中提供稳定、准确的风险说明。
- 执行时仍通过 `ToolPathPolicy` 解析路径；写工具在真正写入前获取 `context.workspace_write_lock().await`。

workspace 内外访问由 `PermissionMode` 与统一 `tool_dispatch` 决定；不要为单个工具新增 `auto allow`、旧审批事件或兼容 policy。

### 6. 交互和运行时事件（按需）

- 需要用户输入时，优先复用 `request_user_input`。确实需要新交互类型时，扩展 `pl-protocol/src/interaction.rs` 的 typed payload/resolution，并同步 Studio bridge、projection 与设计文档。
- artifact、结束当前 turn 等工具结果语义放入 `ToolOutput.runtime_events`，复用 `ToolRuntimeEvent`。
- 不新增工具私有 callback；宿主交互统一从 `TurnOptions::interaction_callback` 进入。

### 7. `pl-core/src/lib.rs` — 稳定边界导出

crate 根通过 `pub use tool::*;` 聚合工具公共 API。只有上层 crate 确实需要直接构造的类型才在
工具模块中声明为 `pub`；内部实现和输入类型默认保持私有或 `pub(crate)`，不要依赖逐项 re-export
隐藏不应公开的 item。

## 现有工具参考

主要内置工具按模块分组，可供参考：

| 模块 | 工具 | 特点 |
|------|------|------|
| **workspace_file/**、**file/** | `read_file`, `list_files`, `apply_patch` 等 | 共享 schema + local/host backend；内容/文件搜索使用 `exec` + `rg`/`rg --files`，写操作持有 workspace 写锁 |
| **exec/mod.rs** | `exec`, `write_stdin` | 唯一命令工具协议，共享 `CommandProcessManager`；`command/` 是 backend 与进程管理实现 |
| **skill/** | `skills_list`, `skill_view`, `skill_manage` | 技能目录访问，严格输入解析 |
| **agent_runtime/collaboration.rs** | `spawn_agent`, `report_progress`, `send_message`, `interrupt_agent`, `list_agents`, `wait_agents`, `read_agent_session`, `close_agent` | 子代理树、显式进度和事件驱动等待 |
| **ask_user.rs** | `request_user_input` | 通过 typed interaction 等待结构化用户输入 |
| **lsp.rs** | `lsp_query_*` | 按语言动态注册的 LSP 代码智能查询（定义跳转、引用查找），依赖 `pl-lsp` crate |

## 条件与动态工具注册

部分工具依赖可选运行时，只能通过当前工具集合组装：

- `ToolSetBuilder::from_capabilities()` 使用本地 backend；`host_provided()` 只使用宿主显式注入的 backend。
- LSP 在 registry 可用时由 `register_lsp_languages()` 同步语言工具；工具名采用 `lsp_query_<language_id>`。
- MCP 工具和 resource façade 由 `McpTurnLease` 直接构造 `RegisteredTool` 并注册到唯一 `ToolRegistry`；租约或服务变化时更新当前工具集合，不保留 MCP 专用 backend 或平行入口。
- `ToolVisibilitySet` 决定本轮模型可见工具，`AgentExecutionPolicy` 再按 tool name 和 `ToolEffect` 收紧执行权限。可见性与执行授权必须同时满足。

## 测试模式

- **保留关键节点**：测试输入业务校验、workspace 越界、写锁、取消、backend 错误映射、运行时事件和注册/权限边界。
- **不测外部库本身**：不为 serde 正常 round-trip、逐工具 JSON Schema `properties`/`required`
  形状、常量 getter 或简单工具名映射重复写测试；Schema 形状只在统一 typed 生成器和真实 wire
  边界集中验证。
- **注册测试**：通过 `TurnEngine::default_provider()` 与 `register_default_tools()` 验证工具是否进入唯一注册链及 capability 关闭行为。
- **交互测试**：使用 typed `InteractionCallback` 验证请求、回答、取消和结束回合语义，不测试旧事件兼容形状。

## 跨 crate 协调提示

- `Tool` 是 dyn-compatible trait；`execute()` 返回 `BoxFuture<'a, ...>`，不要引入 `#[async_trait]`。
- 写工具必须在执行前调用 `context.workspace_write_lock().await` 获取互斥锁。
- `ToolContext` 只消费 canonical turn/runtime 状态；不要向它加入旧字段、原始 JSON projection 或产品专用兼容入口。
- 优先用 `ToolOutput::json()`、`ToolOutput::from_model_output()` 或 `ToolExecutionResult` 构造输出；命令工具的完整输出文件由 `CommandProcessManager` 管理，不是所有工具的固定契约。
- 工具本身不处理审批，也不发送旧审批事件；`tool_dispatch` 统一执行 effect、visibility、路径访问与 interaction 审批。
