use std::fmt;
use std::future::Future;
use std::sync::Arc;

use pl_model::ToolSchema;
use pl_protocol::{PureError, SkillActivation};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::turn::ToolEffect;

use super::contract::{BoxFuture, RegisteredToolFuture, RegisteredToolHandler};
use super::{
    RegisteredToolSchemaError, Tool, ToolContext, ToolExecutionResult, ToolInput, ToolOutput,
    ToolRuntimeLockPolicy,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ToolRuntimeEvent {
    SkillActivated { activation: SkillActivation },
    ToolResultRevision { revision: u64 },
    OutputArtifacts { artifacts: Vec<serde_json::Value> },
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
    supports_parallel_tool_calls: bool,
    runtime_lock_policy: Option<ToolRuntimeLockPolicy>,
    effect: Option<ToolEffect>,
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
            supports_parallel_tool_calls: false,
            runtime_lock_policy: None,
            effect: None,
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
        Artifact: Send + 'static,
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
        Artifact: Send + 'static,
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
        Artifact: Send + 'static,
        Error: fmt::Display + Send + 'static,
    {
        match schema {
            ToolSchema::Function {
                name,
                description,
                input_schema,
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
            } => {
                let _ = (description, format);
                Err(RegisteredToolSchemaError { name })
            }
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
        Artifact: Send + 'static,
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

    pub fn with_runtime_lock_policy(mut self, policy: ToolRuntimeLockPolicy) -> Self {
        self.runtime_lock_policy = Some(policy);
        self
    }

    pub fn with_effect(mut self, effect: ToolEffect) -> Self {
        self.effect = Some(effect);
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

    fn supports_parallel_tool_calls(&self) -> bool {
        self.supports_parallel_tool_calls
    }

    fn effect(&self) -> Option<ToolEffect> {
        self.effect
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
}
