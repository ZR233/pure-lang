use std::fmt;
use std::sync::Arc;

use pl_protocol::{PureError, ToolSpec};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;

use crate::turn::ToolEffect;

use super::cache::ToolCachePolicy;
use super::{
    ToolBatchPolicy, ToolCallContext, ToolDisplayMetadata, ToolInput, ToolResult,
    ToolRuntimeLockPolicy,
};

/// 便捷类型别名：boxed future（来自 `futures` crate 的 `BoxFuture`）。
/// `tool/mod.rs` 以 `pub use futures::future::BoxFuture` 对外暴露同名入口。
type BoxFuture<'a, T> = futures::future::BoxFuture<'a, T>;
pub(super) type LocalToolFuture = BoxFuture<'static, Result<ToolResult, PureError>>;
pub(super) type LocalToolHandler =
    dyn Fn(ToolInput, ToolCallContext) -> LocalToolFuture + Send + Sync;

/// 工具执行区间如何计入 turn 的活跃 wall-clock 预算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolBudgetTiming {
    /// 工具执行时间计入活跃预算。
    Count,
    /// 仅当该工具是批次中唯一成功调度的调用时暂停活跃预算。
    PauseWhenOnlyScheduledTool,
}

/// 工具调用由哪一侧执行。
///
/// `Local` 工具由 [`crate::ToolManager`] 在冻结的 [`crate::ToolPlan`] 中回查执行器；
/// `ProviderHosted` 只把定义发送给 provider，不允许落入本地 dispatch。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolExecution {
    Local,
    ProviderHosted,
}

/// 从静态 Rust 输入类型生成严格的 function tool 输入 schema。
///
/// 字段名、必填性、枚举与说明由 Serde/Schemars typed definition 提供；这里仅统一
/// 移除不属于 provider tool contract 的 root 元数据，并把静态 function 输入规范为
/// provider 要求的 object 根。普通 struct 同时关闭未知顶层字段；object union 的字段
/// 约束保留在各 typed 分支，避免无 root properties 时拒绝所有合法字段。
pub fn typed_tool_input_schema<Input>() -> serde_json::Value
where
    Input: JsonSchema,
{
    let mut schema = schemars::schema_for!(Input).to_value();
    let Some(object) = schema.as_object_mut() else {
        return schema;
    };

    object.remove("$schema");
    object.remove("title");
    let has_object_type = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| value == "object");
    let has_properties = object.contains_key("properties");
    let is_object_union = is_object_union_schema(object);
    let is_object = has_object_type || has_properties || is_object_union;
    if is_object {
        object
            .entry("type")
            .or_insert_with(|| serde_json::Value::String("object".to_string()));
        if !is_object_union || has_properties {
            object.insert(
                "additionalProperties".to_string(),
                serde_json::Value::Bool(false),
            );
        }
    }
    schema
}

fn is_object_schema(schema: &serde_json::Value) -> bool {
    let Some(object) = schema.as_object() else {
        return false;
    };
    object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|value| value == "object")
        || object.contains_key("properties")
}

fn is_object_union_schema(object: &serde_json::Map<String, serde_json::Value>) -> bool {
    ["oneOf", "anyOf"].into_iter().any(|keyword| {
        object
            .get(keyword)
            .and_then(serde_json::Value::as_array)
            .is_some_and(|variants| !variants.is_empty() && variants.iter().all(is_object_schema))
    })
}

/// 将模型 arguments 解析为静态工具输入，并统一拒绝未知顶层字段。
///
/// 对普通输入，Serde 的 `deny_unknown_fields` 仍是类型本身的约束；额外的 root
/// properties 检查覆盖 `#[serde(flatten)]` 组合输入无法同时使用该属性的场景。
pub fn deserialize_tool_input<Input>(
    tool_name: &str,
    arguments: serde_json::Value,
) -> Result<Input, PureError>
where
    Input: DeserializeOwned + JsonSchema,
{
    reject_unknown_tool_input_fields::<Input>(tool_name, &arguments)?;
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: tool_name.to_string(),
        error: format!("invalid input: {error}"),
    })
}

fn reject_unknown_tool_input_fields<Input>(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Result<(), PureError>
where
    Input: JsonSchema,
{
    let Some(arguments) = arguments.as_object() else {
        return Ok(());
    };
    let schema = typed_tool_input_schema::<Input>();
    let Some(schema) = schema.as_object() else {
        return Ok(());
    };
    let properties = schema
        .get("properties")
        .and_then(serde_json::Value::as_object);
    if properties.is_none() && is_object_union_schema(schema) {
        return Ok(());
    }
    let Some(unknown) = arguments
        .keys()
        .find(|key| !properties.is_some_and(|properties| properties.contains_key(*key)))
    else {
        return Ok(());
    };

    let mut expected = properties
        .into_iter()
        .flat_map(serde_json::Map::keys)
        .cloned()
        .collect::<Vec<_>>();
    expected.sort();
    let expected = if expected.is_empty() {
        "no fields".to_string()
    } else {
        format!("one of {}", expected.join(", "))
    };
    Err(PureError::ToolExecutionFailed {
        tool: tool_name.to_string(),
        error: format!("invalid input: unknown field `{unknown}`, expected {expected}"),
    })
}

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
/// `ToolCallContext` 只提供调用身份、取消、事件出口与调用级审批/交互能力；workspace、
/// LSP、产品 session 等稳定依赖由具体工具在构造时捕获。
/// 具体实现中可用 `async move { ... }.boxed()` 包裹异步逻辑。
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
    /// 声明该本地工具可由 provider-hosted programmatic coordinator 调用。
    ///
    /// manager 只接受同时声明 `Read` effect 的工具；第三方 annotation、名称和
    /// provider capability 都不能隐式提升这一资格。
    fn supports_programmatic_calls(&self) -> bool {
        false
    }
    /// 返回该工具执行时间的 turn 活跃预算计时策略。
    fn budget_timing(&self) -> ToolBudgetTiming {
        ToolBudgetTiming::Count
    }
    /// 声明工具对运行环境的副作用类别；无法声明可信 effect 的工具返回 `None`
    /// （独占执行、不可并行、不可 programmatic、结果不缓存）。
    fn effect(&self) -> Option<ToolEffect>;
    fn cache_policy(&self, _arguments: &serde_json::Value) -> ToolCachePolicy {
        ToolCachePolicy::Never
    }
    fn invalidates_cache(&self, _arguments: &serde_json::Value) -> bool {
        false
    }
    fn batch_policy(&self) -> ToolBatchPolicy {
        ToolBatchPolicy::Coexist
    }
    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        if self.supports_parallel_tool_calls() {
            ToolRuntimeLockPolicy::Shared
        } else {
            ToolRuntimeLockPolicy::Exclusive
        }
    }

    /// 返回工具的执行所有权。
    fn execution(&self) -> ToolExecution {
        ToolExecution::Local
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>>;

    /// 返回 provider 无关、模型可见的不可变定义。
    ///
    /// manager 在注册时冻结该值；后续模型 step 与调用调度都只消费冻结结果。
    fn spec(&self) -> ToolSpec {
        ToolSpec::function(self.name(), self.description(), self.input_schema())
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

    fn supports_programmatic_calls(&self) -> bool {
        (**self).supports_programmatic_calls()
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

    fn batch_policy(&self) -> ToolBatchPolicy {
        (**self).batch_policy()
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        (**self).runtime_lock_policy()
    }

    fn execution(&self) -> ToolExecution {
        (**self).execution()
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolCallContext,
    ) -> BoxFuture<'a, Result<ToolResult, PureError>> {
        (**self).execute(input, context)
    }

    fn spec(&self) -> ToolSpec {
        (**self).spec()
    }
}
