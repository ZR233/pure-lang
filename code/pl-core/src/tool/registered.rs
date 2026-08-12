use std::fmt;
use std::future::Future;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::{InteractionRequest, PureError, SkillActivation};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::turn::ToolEffect;

use super::contract::{BoxFuture, RegisteredToolFuture, RegisteredToolHandler};
use super::{
    RegisteredToolSchemaError, Tool, ToolCachePolicy, ToolContext, ToolExecutionResult, ToolInput,
    ToolOutput, ToolRuntimeLockPolicy,
};

type CachePolicyResolver = Arc<dyn Fn(&serde_json::Value) -> ToolCachePolicy + Send + Sync>;
type CacheInvalidationResolver = Arc<dyn Fn(&serde_json::Value) -> bool + Send + Sync>;

/// 与执行授权隔离的工具展示元数据。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolDisplayMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotations: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ToolRuntimeEvent {
    /// 持久化一个由当前 Turn 发起、需要宿主后续处理的交互。
    InteractionRequested {
        interaction: Box<InteractionRequest>,
    },
    SkillActivated {
        activation: SkillActivation,
    },
    ToolResultRevision {
        revision: u64,
    },
    OutputArtifacts {
        artifacts: Vec<serde_json::Value>,
    },
    /// 与模型可见文本分离的 typed 审计数据。
    AuditMetadata {
        metadata: serde_json::Value,
    },
    /// handler 已产生完整输出，但该输出表示一次模型可恢复的工具失败。
    ExecutionFailed,
    CacheHit {
        reused_from_call_id: String,
        result_hash: String,
        total_bytes: u64,
    },
    OutputMetrics {
        raw_bytes: u64,
        model_visible_bytes: u64,
        artifact_bytes: u64,
        result_hash: String,
    },
    /// 声明该工具输出需要越过默认 12KB 安全阈值的硬字节上限。
    ///
    /// 只读概览工具（如 `read_agent_submissions`、`read_review_round`、`task_status`）
    /// 用它保证结构化结果完整投影给模型；仍应配合分页控制总体体积。
    OutputBudget {
        max_bytes: usize,
    },
    EndTurn,
}

/// 运行时动态注册的工具。
///
/// 宿主产品用它把自身业务 handler 挂入 pl-core 的统一 registry 和 dispatch；
/// handler 只负责业务副作用，工具生命周期、trace、权限和 tool result history
/// 仍由 pl-core 统一处理。
#[derive(Clone)]
pub struct RegisteredTool {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    output_schema: Option<serde_json::Value>,
    display_metadata: Option<ToolDisplayMetadata>,
    supports_parallel_tool_calls: bool,
    runtime_lock_policy: Option<ToolRuntimeLockPolicy>,
    effect: Option<ToolEffect>,
    cache_policy: ToolCachePolicy,
    cache_policy_resolver: Option<CachePolicyResolver>,
    cache_invalidation_resolver: Option<CacheInvalidationResolver>,
    handler: Arc<RegisteredToolHandler>,
}

impl fmt::Debug for RegisteredTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RegisteredTool")
            .field("name", &self.name)
            .field(
                "supports_parallel_tool_calls",
                &self.supports_parallel_tool_calls,
            )
            .field("runtime_lock_policy", &self.runtime_lock_policy)
            .finish_non_exhaustive()
    }
}

impl RegisteredTool {
    pub fn new<F, Fut>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInput, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolOutput, PureError>> + Send + 'static,
    {
        let name = name.into();
        let tool_name = name.clone();
        Self {
            name,
            description: description.into(),
            input_schema,
            output_schema: None,
            display_metadata: None,
            supports_parallel_tool_calls: false,
            runtime_lock_policy: None,
            effect: None,
            cache_policy: ToolCachePolicy::Never,
            cache_policy_resolver: None,
            cache_invalidation_resolver: None,
            handler: Arc::new(move |input, context| {
                if context
                    .options
                    .cancellation_token
                    .as_ref()
                    .is_some_and(|token| token.is_cancelled())
                {
                    let tool = tool_name.clone();
                    return Box::pin(async move {
                        Err(PureError::ToolExecutionFailed {
                            tool,
                            error: "tool execution cancelled".to_string(),
                        })
                    }) as RegisteredToolFuture;
                }
                Box::pin(handler(input, context)) as RegisteredToolFuture
            }),
        }
    }

    pub fn from_execution_result<F, Fut, Artifact>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInput, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<ToolExecutionResult<Artifact>, PureError>> + Send + 'static,
        Artifact: Serialize + Send + 'static,
    {
        Self::new(name, description, input_schema, move |input, context| {
            let future = handler(input, context);
            async move { future.await.map(ToolExecutionResult::into_tool_output) }
        })
    }

    pub fn from_fallible_execution_result<F, Fut, Artifact, Error>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        F: Fn(ToolInput, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<ToolExecutionResult<Artifact>, Error>>
            + Send
            + 'static,
        Artifact: Serialize + Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        let name = name.into();
        let tool_name = name.clone();
        Self::new(name, description, input_schema, move |input, context| {
            let future = handler(input, context);
            let tool_name = tool_name.clone();
            async move {
                future
                    .await
                    .map(ToolExecutionResult::into_tool_output)
                    .map_err(|error| PureError::ToolExecutionFailed {
                        tool: tool_name,
                        error: error.to_string(),
                    })
            }
        })
    }

    /// 从模型可见 function schema 注册带强类型输入的产品工具。
    ///
    /// 产品层只需传入自己已经声明的 `ToolSchema` 和业务 handler；pl-core 统一
    /// 解包 function schema 的 name/description/input schema，并复用 typed
    /// 输入解析、错误映射和 `ToolExecutionResult` 输出投影。
    pub fn from_schema_typed_fallible_execution_result<Input, F, Fut, Artifact, Error>(
        schema: ToolSchema,
        handler: F,
    ) -> std::result::Result<Self, RegisteredToolSchemaError>
    where
        Input: DeserializeOwned + Send + 'static,
        F: Fn(Input, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<ToolExecutionResult<Artifact>, Error>>
            + Send
            + 'static,
        Artifact: Serialize + Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        match schema {
            ToolSchema::Function {
                name,
                description,
                input_schema,
                ..
            } => Ok(Self::from_typed_fallible_execution_result(
                name,
                description,
                input_schema,
                handler,
            )),
            ToolSchema::Custom {
                name,
                description,
                format,
                ..
            } => {
                let _ = (description, format);
                Err(RegisteredToolSchemaError { name })
            }
            ToolSchema::Namespace { name, .. } => Err(RegisteredToolSchemaError { name }),
            ToolSchema::ToolSearch => Err(RegisteredToolSchemaError {
                name: "tool_search".to_string(),
            }),
            ToolSchema::ProgrammaticToolCalling => Err(RegisteredToolSchemaError {
                name: "programmatic_tool_calling".to_string(),
            }),
            ToolSchema::WebSearch { .. } => Err(RegisteredToolSchemaError {
                name: "web_search".to_string(),
            }),
        }
    }

    /// 注册带强类型输入的产品工具。
    ///
    /// 宿主只提供产品输入类型和业务 handler；`pl-core` 负责把模型传入的
    /// JSON arguments 反序列化为该类型，并把输入解析错误、业务错误和
    /// `ToolExecutionResult` 统一映射成 canonical `ToolOutput`。
    pub fn from_typed_fallible_execution_result<Input, F, Fut, Artifact, Error>(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: serde_json::Value,
        handler: F,
    ) -> Self
    where
        Input: DeserializeOwned + Send + 'static,
        F: Fn(Input, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = std::result::Result<ToolExecutionResult<Artifact>, Error>>
            + Send
            + 'static,
        Artifact: Serialize + Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        let name = name.into();
        let tool_name = name.clone();
        Self::new(name, description, input_schema, move |input, context| {
            let tool_name = tool_name.clone();
            let arguments = match serde_json::from_value::<Input>(input.arguments) {
                Ok(arguments) => arguments,
                Err(error) => {
                    return Box::pin(async move {
                        Err(PureError::ToolExecutionFailed {
                            tool: tool_name,
                            error: format!("invalid input: {error}"),
                        })
                    }) as RegisteredToolFuture;
                }
            };
            let future = handler(arguments, context);
            Box::pin(async move {
                future
                    .await
                    .map(ToolExecutionResult::into_tool_output)
                    .map_err(|error| PureError::ToolExecutionFailed {
                        tool: tool_name,
                        error: error.to_string(),
                    })
            }) as RegisteredToolFuture
        })
    }

    pub fn with_parallel_tool_calls(mut self) -> Self {
        self.supports_parallel_tool_calls = true;
        self
    }

    /// 保留 provider 可见的工具输出 JSON Schema。
    pub fn with_output_schema(mut self, output_schema: Option<serde_json::Value>) -> Self {
        self.output_schema = output_schema;
        self
    }

    pub fn with_runtime_lock_policy(mut self, policy: ToolRuntimeLockPolicy) -> Self {
        self.runtime_lock_policy = Some(policy);
        self
    }

    pub fn with_effect(mut self, effect: ToolEffect) -> Self {
        self.effect = Some(effect);
        self
    }

    pub fn with_display_metadata(mut self, metadata: ToolDisplayMetadata) -> Self {
        self.display_metadata = Some(metadata);
        self
    }

    pub fn with_cache_policy(mut self, policy: ToolCachePolicy) -> Self {
        self.cache_policy = policy;
        self.cache_policy_resolver = None;
        self
    }

    /// 按规范化工具参数选择 turn-scoped 缓存策略。
    ///
    /// 用于同一产品工具同时承载只读与写入操作的场景；resolver 必须是纯函数，
    /// 不得读取外部状态或执行 I/O。
    pub fn with_cache_policy_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&serde_json::Value) -> ToolCachePolicy + Send + Sync + 'static,
    {
        self.cache_policy_resolver = Some(Arc::new(resolver));
        self
    }

    /// 声明哪些成功调用会使同名只读结果失效。
    pub fn with_cache_invalidation_resolver<F>(mut self, resolver: F) -> Self
    where
        F: Fn(&serde_json::Value) -> bool + Send + Sync + 'static,
    {
        self.cache_invalidation_resolver = Some(Arc::new(resolver));
        self
    }
}

impl Tool for RegisteredTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    fn display_metadata(&self) -> Option<&ToolDisplayMetadata> {
        self.display_metadata.as_ref()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.supports_parallel_tool_calls
    }

    fn effect(&self) -> Option<ToolEffect> {
        self.effect
    }

    fn cache_policy(&self, arguments: &serde_json::Value) -> ToolCachePolicy {
        self.cache_policy_resolver
            .as_ref()
            .map_or(self.cache_policy, |resolver| resolver(arguments))
    }

    fn invalidates_cache(&self, arguments: &serde_json::Value) -> bool {
        self.cache_invalidation_resolver
            .as_ref()
            .is_some_and(|resolver| resolver(arguments))
    }

    fn runtime_lock_policy(&self) -> ToolRuntimeLockPolicy {
        self.runtime_lock_policy.unwrap_or({
            if self.supports_parallel_tool_calls {
                ToolRuntimeLockPolicy::Shared
            } else {
                ToolRuntimeLockPolicy::Exclusive
            }
        })
    }

    fn execute<'a>(
        &'a self,
        input: ToolInput,
        context: ToolContext,
    ) -> BoxFuture<'a, Result<ToolOutput, PureError>> {
        (self.handler)(input, context)
    }

    fn to_schema(&self) -> ToolSchema {
        ToolSchema::Function {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            defer_loading: false,
            allowed_callers: Vec::new(),
            output_schema: self.output_schema.clone(),
        }
    }
}
