use std::any::TypeId;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

use pl_protocol::{PureError, Result, ToolSpec};
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
type CachePolicyResolver = Arc<dyn Fn(&serde_json::Value) -> ToolCachePolicy + Send + Sync>;
type CacheInvalidationResolver = Arc<dyn Fn(&serde_json::Value) -> bool + Send + Sync>;

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
    /// Core dispatches the invocation through the frozen executor.
    Local,
    /// The model provider executes the tool; core only sends its definition.
    ProviderHosted,
}

/// A validated internal identity and its provider-visible wire name.
///
/// Namespaced tools retain their structured identity inside core. Providers,
/// traces and persisted history continue to use [`Self::wire_name`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToolName {
    namespace: Option<Arc<str>>,
    name: Arc<str>,
    wire_name: Arc<str>,
}

impl ToolName {
    pub(crate) fn builtin(name: &'static str) -> Self {
        Self {
            namespace: None,
            name: Arc::from(name),
            wire_name: Arc::from(name),
        }
    }

    /// Creates a tool without an internal namespace.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when the name is empty.
    pub fn bare(name: impl AsRef<str>) -> Result<Self> {
        let name = validate_name_component("tool name", name.as_ref())?;
        Ok(Self {
            namespace: None,
            name: Arc::from(name),
            wire_name: Arc::from(name),
        })
    }

    /// Creates a namespaced tool whose wire name is `namespace__name`.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when either component is empty.
    pub fn namespaced(namespace: impl AsRef<str>, name: impl AsRef<str>) -> Result<Self> {
        let namespace = validate_name_component("tool namespace", namespace.as_ref())?;
        let name = validate_name_component("tool name", name.as_ref())?;
        let wire_name = format!("{namespace}__{name}");
        Ok(Self {
            namespace: Some(Arc::from(namespace)),
            name: Arc::from(name),
            wire_name: Arc::from(wire_name),
        })
    }

    /// Creates a structured identity with an externally assigned stable wire name.
    ///
    /// This is intended for adapters such as MCP whose existing collision-safe
    /// wire naming algorithm must remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when any component is empty.
    pub fn with_wire_name(
        namespace: impl AsRef<str>,
        name: impl AsRef<str>,
        wire_name: impl AsRef<str>,
    ) -> Result<Self> {
        let namespace = validate_name_component("tool namespace", namespace.as_ref())?;
        let name = validate_name_component("tool name", name.as_ref())?;
        let wire_name = validate_name_component("tool wire name", wire_name.as_ref())?;
        Ok(Self {
            namespace: Some(Arc::from(namespace)),
            name: Arc::from(name),
            wire_name: Arc::from(wire_name),
        })
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    /// Returns the source-local name without namespace flattening.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the provider-visible stable name.
    pub fn wire_name(&self) -> &str {
        &self.wire_name
    }
}

impl fmt::Display for ToolName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.wire_name())
    }
}

fn validate_name_component<'a>(label: &str, value: &'a str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        return Err(PureError::ConfigError(format!("{label} cannot be empty")));
    }
    Ok(value)
}

/// Definition supplied by a statically typed Rust tool.
#[derive(Debug, Clone)]
pub struct StaticToolDefinition {
    name: ToolName,
    description: Arc<str>,
    output_schema: Option<serde_json::Value>,
    display_metadata: Option<ToolDisplayMetadata>,
}

impl StaticToolDefinition {
    /// Creates a static definition from a validated identity and model-facing purpose.
    pub fn new(name: ToolName, description: impl Into<Arc<str>>) -> Self {
        Self {
            name,
            description: description.into(),
            output_schema: None,
            display_metadata: None,
        }
    }

    /// Attaches an optional provider-facing output schema.
    pub fn with_output_schema(mut self, output_schema: serde_json::Value) -> Self {
        self.output_schema = Some(output_schema);
        self
    }

    /// Attaches presentation-only metadata that cannot change execution policy.
    pub fn with_display_metadata(mut self, metadata: ToolDisplayMetadata) -> Self {
        self.display_metadata = Some(metadata);
        self
    }

    /// Returns the validated structured identity.
    pub fn name(&self) -> &ToolName {
        &self.name
    }

    /// Returns the model-facing overall purpose.
    pub fn description(&self) -> &str {
        &self.description
    }
}

/// Immutable model-visible definition owned by a type-erased tool.
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    name: ToolName,
    spec: ToolSpec,
    display_metadata: Option<ToolDisplayMetadata>,
}

impl ToolDefinition {
    /// Projects a static definition and input schema into a function tool definition.
    pub fn function(
        static_definition: StaticToolDefinition,
        input_schema: serde_json::Value,
    ) -> Self {
        let spec = ToolSpec::Function {
            name: static_definition.name.wire_name().to_string(),
            description: static_definition.description.to_string(),
            input_schema,
            allowed_callers: Vec::new(),
            output_schema: static_definition.output_schema,
        };
        Self {
            name: static_definition.name,
            spec,
            display_metadata: static_definition.display_metadata,
        }
    }

    /// Wraps an externally supplied spec after checking its stable name.
    ///
    /// # Errors
    ///
    /// Returns [`PureError::ConfigError`] when `name` and `spec` disagree.
    pub fn from_spec(
        name: ToolName,
        spec: ToolSpec,
        display_metadata: Option<ToolDisplayMetadata>,
    ) -> Result<Self> {
        if name.wire_name() != spec.name() {
            return Err(PureError::ConfigError(format!(
                "tool identity `{}` does not match spec name `{}`",
                name.wire_name(),
                spec.name()
            )));
        }
        Ok(Self {
            name,
            spec,
            display_metadata,
        })
    }

    pub(crate) fn from_trusted_spec(name: ToolName, spec: ToolSpec) -> Self {
        Self {
            name,
            spec,
            display_metadata: None,
        }
    }

    /// Returns the structured identity retained by core.
    pub fn name(&self) -> &ToolName {
        &self.name
    }

    /// Returns the provider-facing immutable specification.
    pub fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    /// Returns presentation-only metadata, when supplied.
    pub fn display_metadata(&self) -> Option<&ToolDisplayMetadata> {
        self.display_metadata.as_ref()
    }
}

/// Trusted execution policy, kept separate from external display metadata.
#[derive(Clone)]
pub struct ToolPolicy {
    effect: Option<ToolEffect>,
    supports_parallel_tool_calls: bool,
    supports_programmatic_calls: bool,
    budget_timing: ToolBudgetTiming,
    batch_policy: ToolBatchPolicy,
    runtime_lock_policy: Option<ToolRuntimeLockPolicy>,
    cache_policy: ToolCachePolicy,
    cache_policy_resolver: Option<CachePolicyResolver>,
    cache_invalidation_resolver: Option<CacheInvalidationResolver>,
}

impl fmt::Debug for ToolPolicy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolPolicy")
            .field("effect", &self.effect)
            .field(
                "supports_parallel_tool_calls",
                &self.supports_parallel_tool_calls,
            )
            .field(
                "supports_programmatic_calls",
                &self.supports_programmatic_calls,
            )
            .field("budget_timing", &self.budget_timing)
            .field("batch_policy", &self.batch_policy)
            .field("runtime_lock_policy", &self.runtime_lock_policy())
            .field("cache_policy", &self.cache_policy)
            .finish_non_exhaustive()
    }
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            effect: None,
            supports_parallel_tool_calls: false,
            supports_programmatic_calls: false,
            budget_timing: ToolBudgetTiming::Count,
            batch_policy: ToolBatchPolicy::Coexist,
            runtime_lock_policy: None,
            cache_policy: ToolCachePolicy::Never,
            cache_policy_resolver: None,
            cache_invalidation_resolver: None,
        }
    }
}

impl ToolPolicy {
    /// Creates the standard read-only policy.
    pub fn read_only() -> Self {
        Self::default().with_effect(ToolEffect::Read)
    }

    /// Declares the tool's trusted side effect.
    pub fn with_effect(mut self, effect: ToolEffect) -> Self {
        self.effect = Some(effect);
        self
    }

    /// Allows independent calls to execute concurrently.
    pub fn with_parallel_tool_calls(mut self) -> Self {
        self.supports_parallel_tool_calls = true;
        self
    }

    /// Allows the hosted programmatic coordinator to call this tool.
    pub fn with_programmatic_calls(mut self) -> Self {
        self.supports_programmatic_calls = true;
        self
    }

    /// Selects how execution contributes to active wall-clock budget.
    pub fn with_budget_timing(mut self, timing: ToolBudgetTiming) -> Self {
        self.budget_timing = timing;
        self
    }

    /// Selects whether the tool may coexist with other calls in one batch.
    pub fn with_batch_policy(mut self, policy: ToolBatchPolicy) -> Self {
        self.batch_policy = policy;
        self
    }

    /// Overrides the runtime lock derived from the parallel-call flag.
    pub fn with_runtime_lock_policy(mut self, policy: ToolRuntimeLockPolicy) -> Self {
        self.runtime_lock_policy = Some(policy);
        self
    }

    /// Installs a fixed result-cache policy.
    pub fn with_cache_policy(mut self, policy: ToolCachePolicy) -> Self {
        self.cache_policy = policy;
        self.cache_policy_resolver = None;
        self
    }

    /// Resolves result-cache policy from one invocation's arguments.
    pub fn with_cache_policy_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&serde_json::Value) -> ToolCachePolicy + Send + Sync + 'static,
    {
        self.cache_policy_resolver = Some(Arc::new(resolver));
        self
    }

    /// Declares argument-dependent cache invalidation.
    pub fn with_cache_invalidation_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&serde_json::Value) -> bool + Send + Sync + 'static,
    {
        self.cache_invalidation_resolver = Some(Arc::new(resolver));
        self
    }

    /// Returns the trusted effect, if one was declared.
    pub fn effect(&self) -> Option<ToolEffect> {
        self.effect
    }

    /// Returns whether independent calls may run concurrently.
    pub fn supports_parallel_tool_calls(&self) -> bool {
        self.supports_parallel_tool_calls
    }

    /// Returns whether hosted programmatic calling may select this tool.
    pub fn supports_programmatic_calls(&self) -> bool {
        self.supports_programmatic_calls
    }

    /// Returns how execution contributes to active wall-clock budget.
    pub fn budget_timing(&self) -> ToolBudgetTiming {
        self.budget_timing
    }

    /// Returns the batch coexistence policy.
    pub fn batch_policy(&self) -> ToolBatchPolicy {
        self.batch_policy
    }

    /// Returns the explicit or parallelism-derived runtime lock policy.
    pub fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        self.runtime_lock_policy
            .unwrap_or(if self.supports_parallel_tool_calls {
                ToolRuntimeLockPolicy::Shared
            } else {
                ToolRuntimeLockPolicy::Exclusive
            })
    }

    /// Resolves cache policy for one invocation.
    pub fn cache_policy(&self, arguments: &serde_json::Value) -> ToolCachePolicy {
        self.cache_policy_resolver
            .as_ref()
            .map_or(self.cache_policy, |resolver| resolver(arguments))
    }

    /// Returns whether one successful invocation invalidates cached entries.
    pub fn invalidates_cache(&self, arguments: &serde_json::Value) -> bool {
        self.cache_invalidation_resolver
            .as_ref()
            .is_some_and(|resolver| resolver(arguments))
    }
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
pub fn deserialize_tool_input<Input>(tool_name: &str, arguments: serde_json::Value) -> Result<Input>
where
    Input: DeserializeOwned + JsonSchema + 'static,
{
    // Runtime-selected static adapters (for example one Git or workspace-file
    // kind) retain the original JSON value and perform their kind-specific
    // typed deserialization in the handler. Their model-visible schema is still
    // supplied by `StaticTool::input_schema`; treating `Value`'s own schema as
    // that contract would reject every object field before dispatch.
    if TypeId::of::<Input>() == TypeId::of::<serde_json::Value>() {
        return serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
            tool: tool_name.to_string(),
            error: format!("invalid input: {error}"),
        });
    }
    reject_unknown_tool_input_fields::<Input>(tool_name, &arguments)?;
    serde_json::from_value(arguments).map_err(|error| PureError::ToolExecutionFailed {
        tool: tool_name.to_string(),
        error: format!("invalid input: {error}"),
    })
}

fn reject_unknown_tool_input_fields<Input>(
    tool_name: &str,
    arguments: &serde_json::Value,
) -> Result<()>
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

/// One invocation passed through the type-erased executor boundary.
pub struct ToolInvocation {
    /// Provider arguments preserved at the type-erased boundary.
    pub input: ToolInput,
    /// Frozen call identity, cancellation, approval and workspace context.
    pub context: ToolCallContext,
}

impl ToolInvocation {
    /// Creates one type-erased invocation.
    pub fn new(input: ToolInput, context: ToolCallContext) -> Self {
        Self { input, context }
    }

    /// Decomposes the invocation into its owned parts.
    pub fn into_parts(self) -> (ToolInput, ToolCallContext) {
        (self.input, self.context)
    }
}

/// Object-safe execution boundary used by every runtime tool source.
pub trait ToolExecutor: fmt::Debug + Send + Sync {
    /// Returns the immutable provider-facing definition.
    fn definition(&self) -> &ToolDefinition;
    /// Returns the trusted execution policy.
    fn policy(&self) -> &ToolPolicy;
    /// Returns whether core or the provider owns execution.
    fn execution(&self) -> ToolExecution;

    /// Executes one invocation through the object-safe boundary.
    fn execute(&self, invocation: ToolInvocation) -> BoxFuture<'_, Result<ToolResult>>;
}

/// Typed Rust authoring boundary. It is intentionally not object-safe.
pub trait StaticTool: fmt::Debug + Send + Sync + 'static {
    /// Strict Rust input contract used for Schema generation and deserialization.
    type Input: DeserializeOwned + JsonSchema + Send + 'static;

    /// Returns the validated identity and model-facing purpose.
    fn definition(&self) -> StaticToolDefinition;

    /// Builds the input schema, normally from [`Self::Input`] through Schemars.
    ///
    /// Runtime-selected built-in adapters may override this when one executor
    /// type exposes several typed input variants.
    fn input_schema(&self) -> serde_json::Value {
        typed_tool_input_schema::<Self::Input>()
    }

    /// Returns trusted effect, concurrency, cache and batch policy.
    fn policy(&self) -> ToolPolicy;

    /// Returns the execution owner; static tools are local by default.
    fn execution(&self) -> ToolExecution {
        ToolExecution::Local
    }

    /// Executes an already-deserialized typed invocation.
    fn execute(
        &self,
        input: Self::Input,
        context: ToolCallContext,
    ) -> impl Future<Output = Result<ToolResult>> + Send;
}

struct StaticToolAdapter<T> {
    tool: T,
    definition: ToolDefinition,
    policy: ToolPolicy,
    execution: ToolExecution,
}

impl<T> StaticToolAdapter<T>
where
    T: StaticTool,
{
    fn new(tool: T) -> Self {
        let definition = ToolDefinition::function(tool.definition(), tool.input_schema());
        let policy = tool.policy();
        let execution = tool.execution();
        Self {
            tool,
            definition,
            policy,
            execution,
        }
    }
}

impl<T> fmt::Debug for StaticToolAdapter<T>
where
    T: StaticTool,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StaticToolAdapter")
            .field("name", &self.definition.name())
            .field("tool", &self.tool)
            .finish_non_exhaustive()
    }
}

impl<T> ToolExecutor for StaticToolAdapter<T>
where
    T: StaticTool,
{
    fn definition(&self) -> &ToolDefinition {
        &self.definition
    }

    fn policy(&self) -> &ToolPolicy {
        &self.policy
    }

    fn execution(&self) -> ToolExecution {
        self.execution
    }

    fn execute(&self, invocation: ToolInvocation) -> BoxFuture<'_, Result<ToolResult>> {
        let (input, context) = invocation.into_parts();
        let tool_name = self.definition.name().wire_name().to_string();
        Box::pin(async move {
            if context.is_cancelled() {
                return Err(PureError::ToolExecutionFailed {
                    tool: tool_name,
                    error: "tool execution cancelled".to_string(),
                });
            }
            let input = deserialize_tool_input::<T::Input>(&tool_name, input.arguments)?;
            self.tool.execute(input, context).await
        })
    }
}

/// The only tool container stored by registries and frozen plans.
#[derive(Clone)]
pub struct DynTool(Arc<dyn ToolExecutor>);

impl DynTool {
    /// Erases one dynamic executor behind the shared `Arc` container.
    pub fn new_executor(executor: impl ToolExecutor + 'static) -> Self {
        Self(Arc::new(executor))
    }

    /// Returns the cached immutable definition.
    pub fn definition(&self) -> &ToolDefinition {
        self.0.definition()
    }

    /// Returns the cached trusted policy.
    pub fn policy(&self) -> &ToolPolicy {
        self.0.policy()
    }

    /// Returns the execution owner.
    pub fn execution(&self) -> ToolExecution {
        self.0.execution()
    }

    /// Executes an invocation without recovering the concrete executor type.
    pub fn execute(&self, invocation: ToolInvocation) -> BoxFuture<'_, Result<ToolResult>> {
        self.0.execute(invocation)
    }
}

impl fmt::Debug for DynTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DynTool")
            .field("name", &self.definition().name())
            .field("policy", self.policy())
            .field("execution", &self.execution())
            .finish_non_exhaustive()
    }
}

impl<T> From<T> for DynTool
where
    T: StaticTool,
{
    fn from(tool: T) -> Self {
        Self::new_executor(StaticToolAdapter::new(tool))
    }
}
