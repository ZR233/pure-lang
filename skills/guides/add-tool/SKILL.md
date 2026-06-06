---
name: add-tool
description: Use when adding a new built-in tool to Pure-Lang. Covers Tool trait implementation, schema definition, registration chain, permission policy, event emission, callback patterns, and cross-crate export.
category: guides
platforms: ["windows", "linux", "macos"]
---

# Add a New Built-in Tool

在 Pure-Lang 中添加新内置工具时，工具系统位于 `pl-core` 内部（`code/pl-core/src/tool/`），目前没有独立的 `pl-tool` crate。所有工具最终通过 `PureCore::register_default_tools()` 注册到 `ToolRegistry`。

## 前置确认

1. **确定工具名**：snake_case，全局唯一（`ToolRegistry::register()` 同名断言防止重复）。
2. **确定输入 schema**：参数通过 `serde_json::Value` 传递；结构化输入类型放在工具文件内。
3. **确定是否支持并行调用**：`supports_parallel_tool_calls()` 默认 `false`。
4. **确定工具类型**：绝大多数工具是 `ToolSchema::Function`（JSON Schema 参数）；`apply_patch` 是唯一的 `ToolSchema::Custom`（Lark grammar）。
5. **确定权限策略**：是否需要审批？在 Plan mode 是否可用？是否需要 `workspace_write_lock`？
6. **确定是否需要回调**：如需要等待用户输入，参考 `ToolApprovalCallback` 模式创建新的回调类型。
7. **确定是否发事件**：需要前端通信时，在 `AgentEvent` 中添加新变体。

## 修改清单

### 1. `pl-core/src/tool/<new_tool>.rs` — Tool 实现

创建新文件，实现 `Tool` trait：

```rust
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pl_protocol::{AgentEvent, PureError};
use serde::{Deserialize, Serialize};

use crate::tool::{ToolContext, ToolInput, ToolOutput};
use crate::Tool;

pub struct MyNewTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MyNewToolInput {
    // 工具参数（可根据需要选择是否去前导下划线）
    pub query: String,
    pub options: Option<Vec<String>>,
}

impl Tool for MyNewTool {
    fn name(&self) -> &str {
        "my_new_tool"
    }

    fn description(&self) -> &str {
        "Description shown to LLM explaining when to use this tool."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "What to search for"
                },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional predefined choices"
                }
            },
            "required": ["query"]
        })
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput, PureError>> + Send + 'a>> {
        Box::pin(async move {
            // 1. 从 input.arguments 中解析参数
            // 2. 需要用户交互时：
            //    - 发送 AgentEvent（如 UserQuestionAsked）
            //    - 调用回调等待用户响应
            // 3. 构造 ToolOutput 返回

            // 发送事件示例：
            let _ = context.event_tx.send(AgentEvent::UserQuestionAsked {
                tool_id: input.tool_id.clone(),
                question: parsed_question,
            });

            // 等待回调示例（需要 ToolEventCallback 类型）：
            // let answer = context.options.user_question_callback(parsed_question).await;

            Ok(ToolOutput {
                description: result_text,
                truncated: Default::default(),
                output_file: PathBuf::new(),
                exit_code: Some(0),
                timed_out: false,
            })
        })
    }
}
```

### 2. `pl-core/src/tool/mod.rs` — 模块声明 + 导出

- 添加 `mod <new_tool>;`
- 添加 `pub use <new_tool>::MyNewTool;`（如果需要，同时也 `pub use` 输入类型）

### 3. `pl-core/src/turn.rs` — 回调类型（仅需要等待用户交互时）

如果工具需要等待用户输入（类似 `AskUserTool`），新增回调类型：

```rust
pub type UserQuestionFuture = Pin<Box<dyn Future<Output = Option<String>> + Send>>;
pub type UserQuestionCallback =
    Arc<dyn Fn(String) -> UserQuestionFuture + Send + Sync>;
```

并在 `TurnOptions` 中添加字段：

```rust
pub user_question_callback: Option<UserQuestionCallback>,
```

### 4. `pl-protocol/src/event.rs` — 事件类型（仅需要前端通信时）

在 `AgentEvent` 枚举中添加新变体：

```rust
UserQuestionAsked {
    tool_id: String,
    question: String,
},
UserQuestionAnswered {
    tool_id: String,
    answer: String,
},
```

### 5. `pl-core/src/core/mod.rs` — 注册到 PureCore

在 `PureCore::register_default_tools()` 中注册：

```rust
self.register_tool(MyNewTool);
```

### 6. `pl-core/src/core/turn_result.rs` — Plan mode 白名单

在 `tool_allowed_in_mode()` 函数中，如果工具在 Plan mode 下应可用，将其加入白名单：

```rust
// 只读/纯信息工具在 Plan mode 可用
"my_new_tool" | "ask_user" | "read_file" | "list_files" // ...
```

### 7. `pl-core/src/permission.rs` — 权限策略

在 `decide_tool_permission()` 中，如果工具不需要审批（纯信息类），添加特殊处理：

```rust
// 纯信息工具直接放行
if matches!(request.name, "my_new_tool" | "ask_user") {
    return PermissionDecision::Approved {
        workspace_access: WorkspaceAccess::WorkspaceOnly,
    };
}
```

### 8. `pl-core/src/lib.rs` — 公开导出

在 `pub use tool::` 块中添加 `MyNewTool`（如果上层需要直接访问）。

## 现有工具参考

所有 21 个内置工具按模块分组，可供参考：

| 模块 | 工具 | 特点 |
|------|------|------|
| **file/read.rs** | `read_file`, `list_files`, `search_files`, `stat_path` | 只读，Plan mode 可用 |
| **file/write.rs** | `write_file`, `create_directory`, `delete_path`, `copy_path`, `move_path` | 写操作，需 `workspace_write_lock()` |
| **file/mod.rs** | `apply_patch` | 唯一的 `ToolSchema::Custom`，Lark grammar |
| **bash.rs** | `bash` | 异步进程执行，截断策略，后台命令支持 |
| **skill.rs** | `skills_list`, `skill_view`, `skill_manage` | 技能目录访问，严格输入解析 |
| **multi_agent/tools.rs** | `spawn_agent`, `wait_agent`, `list_agents`, `send_message`, `followup_task`, `close_agent` | 子代理树管理，429 恢复，生命周期事件转发 |
| **ask_user** (待实现) | `ask_user` | 回调等待用户输入，纯信息收集 |

## 测试模式

- **单元测试**：在工具文件中用 `#[cfg(test)]` 添加，测试输入解析、输出构造、边界条件。
- **集成测试**：通过 `PureCore::default_provider()` 构建 `ToolRegistry`，用 `get()` 获取工具测试执行。
- **回调测试**：使用 mock callback 验证回调被正确调用和响应。

## 跨 crate 协调提示

- `Tool` trait 使用原生 RPITIT，不需要 `#[async_trait]`。`execute()` 返回 `BoxFuture<'a, ...>`（`Pin<Box<dyn Future<...> + Send + 'a>>`）。
- 写工具必须在执行前调用 `context.workspace_write_lock().await` 获取互斥锁。
- `ToolContext` 的字段：`event_tx`、`options`、`workspace_access`、`mode`、`workspace_root`、`workspace_instructions`、`active_subagent`、`agent_control`。
- `ToolOutput` 的 `description` 字段是返回给 LLM 的文本内容（会被截断），`output_file` 路径为 `target/pure/{session_id}/{tool_id}/output.log`。
- 发送 `AgentEvent::ToolApprovalRequested / Granted / Denied` 时，工具本身不处理审批，由 `tool_dispatch.rs` 统一调度。
