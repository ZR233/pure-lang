use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::PureError;

use crate::turn::ToolEffect;

use super::{
    ToolCachePolicy, ToolContext, ToolDisplayMetadata, ToolInput, ToolOutput, ToolRuntimeLockPolicy,
};

/// 便捷类型别名：boxed future。
pub(super) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub(super) type RegisteredToolFuture =
    Pin<Box<dyn Future<Output = Result<ToolOutput, PureError>> + Send>>;
pub(super) type RegisteredToolHandler =
    dyn Fn(ToolInput, ToolContext) -> RegisteredToolFuture + Send + Sync;

/// 工具执行区间如何计入 turn 的活跃 wall-clock 预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolBudgetTiming {
    /// 工具执行时间计入活跃预算。
    Count,
    /// 仅当该工具是批次中唯一成功调度的调用时暂停活跃预算。
    PauseWhenOnlyScheduledTool,
}

/// 严格 object 输入 schema 中的字段。
///
/// 产品层和共享工具都应通过 `required` / `optional` 命名构造器声明字段，
/// 避免在不同仓库里重复维护 `required` 数组和 `additionalProperties` 形状。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolInputSchemaField {
    name: String,
    schema: serde_json::Value,
    required: bool,
}

impl ToolInputSchemaField {
    pub fn required(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
            required: true,
        }
    }

    pub fn optional(name: impl Into<String>, schema: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            schema,
            required: false,
        }
    }
}

/// 构造工具统一使用的严格 object 输入 schema。
pub fn strict_tool_input_schema(
    fields: impl IntoIterator<Item = ToolInputSchemaField>,
) -> serde_json::Value {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for field in fields {
        if field.required {
            required.push(serde_json::Value::String(field.name.clone()));
        }
        properties.insert(field.name, field.schema);
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// 构造 function tool schema，并统一使用严格 object 输入 schema。
pub fn function_tool_schema(
    name: impl Into<String>,
    description: impl Into<String>,
    fields: impl IntoIterator<Item = ToolInputSchemaField>,
) -> ToolSchema {
    ToolSchema::function(name, description, strict_tool_input_schema(fields))
}

/// 动态注册工具 schema 不符合 pl-core typed handler 入口时的错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredToolSchemaError {
    pub(super) name: String,
}

impl RegisteredToolSchemaError {
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for RegisteredToolSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "registered tool `{}` must use a function schema",
            self.name
        )
    }
}

impl std::error::Error for RegisteredToolSchemaError {}

/// 等待宿主工具后端 future，并统一响应 turn cancellation。
///
/// 宿主 adapter 仍负责业务调用和错误类型；pl-core 负责维护工具执行过程中
/// cancellation token 与后台 future 的竞争语义，避免每个产品后端重复手写
/// `tokio::select!`。
pub async fn run_tool_backend_with_cancellation<F, T, E>(
    future: F,
    cancellation_token: Option<tokio_util::sync::CancellationToken>,
    cancelled_error: impl FnOnce() -> E,
) -> std::result::Result<T, E>
where
    F: Future<Output = std::result::Result<T, E>> + Send,
{
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            return Err(cancelled_error());
        }
        return tokio::select! {
            result = future => result,
            _ = token.cancelled() => Err(cancelled_error()),
        };
    }

    future.await
}

/// 工具执行抽象（dyn-compatible）。
///
/// `execute` 返回 `BoxFuture` 以支持 trait object。
/// `ToolContext` 提供事件转发、审批策略和当前 subagent 运行边界。
/// 具体实现中可用 `Box::pin(async move { ... })` 包裹异步逻辑。
pub trait Tool: fmt::Debug + Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> serde_json::Value;
    /// 返回仅用于展示和审计的工具元数据。
    ///
    /// 调度器不得用这些远端声明提升 effect、权限、并行或缓存能力。
    fn display_metadata(&self) -> Option<&ToolDisplayMetadata> {
        None
    }
    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }
    /// 返回该工具执行时间的 turn 活跃预算计时策略。
    fn budget_timing(&self) -> ToolBudgetTiming {
        ToolBudgetTiming::Count
    }
    fn effect(&self) -> Option<ToolEffect> {
        ToolEffect::for_builtin_name(self.name())
    }
    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        match self.name() {
            "read_file" | "list_files" | "stat_path" | "skills_list" | "skill_view"
            | "git_workspace_info" | "git_status" | "git_diff" => {
                ToolCachePolicy::UntilWorkspaceMutation
            }
            _ => ToolCachePolicy::Never,
        }
    }
    fn invalidates_cache(&self, _arguments: &serde_json::Value) -> bool {
        false
    }
    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        if self.supports_parallel_tool_calls() {
            ToolRuntimeLockPolicy::Shared
        } else {
            ToolRuntimeLockPolicy::Exclusive
        }
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>>;

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::function(self.name(), self.description(), self.input_schema())
    }
}

impl<T> Tool for Arc<T>
where
    T: Tool + ?Sized + 'static,
{
    fn name(&self) -> &str {
        (**self).name()
    }

    fn description(&self) -> &str {
        (**self).description()
    }

    fn input_schema(&self) -> serde_json::Value {
        (**self).input_schema()
    }

    fn display_metadata(&self) -> Option<&ToolDisplayMetadata> {
        (**self).display_metadata()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        (**self).supports_parallel_tool_calls()
    }

    fn budget_timing(&self) -> ToolBudgetTiming {
        (**self).budget_timing()
    }

    fn effect(&self) -> Option<ToolEffect> {
        (**self).effect()
    }

    fn cache_policy(&self, arguments: &serde_json::Value) -> ToolCachePolicy {
        (**self).cache_policy(arguments)
    }

    fn invalidates_cache(&self, arguments: &serde_json::Value) -> bool {
        (**self).invalidates_cache(arguments)
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        (**self).runtime_lock_policy()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        (**self).execute(input, context)
    }
}
