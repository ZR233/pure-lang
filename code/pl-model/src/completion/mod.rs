//! Canonical completion 请求、响应、事件、工具与 Web Search 类型。
//!
//! 按域拆分:`request` 承载请求构造与能力校验,`response` 承载响应与 trace 上下文,
//! `tool_call`/`tool_schema` 承载工具调用与 schema,`compaction` 承载远程压缩,
//! `usage` 承载 token 用量与 reasoning 配置,`stream`/`tool_arguments`/
//! `visible_text`/`web_search` 为既有子域。
//!
//! 本域公共类型字段与公共方法签名中出现的 `pl-protocol` 类型在下方精确重导出，
//! 消费方只需依赖 `pl-model` 即可命名完整签名。

pub(crate) mod compaction;
pub(crate) mod request;
pub(crate) mod response;
pub(crate) mod stream;
pub(crate) mod tool_arguments;
pub(crate) mod tool_call;
pub(crate) mod tool_schema;
pub(crate) mod usage;
mod visible_text;

pub use compaction::*;
pub use pl_protocol::{
    AttachmentModality, ContentPart, HostedWebSearchOptions, InferenceAccounting,
    InferenceOrchestrationMetrics, InferenceTiming, InferenceTokenUsage, Message, MessageContent,
    MessageRole, ModelContextItem, PureError, ResponsesContextItem, Result, ToolCallCaller,
    ToolCallKind, ToolCallRecord, ToolSpec, UsageReport, WebSearchContextSize, WebSearchFilters,
    WebSearchUserLocation,
};
pub use request::*;
pub use response::*;
pub use tool_call::*;
pub use usage::*;

mod snapshot;
pub use snapshot::{
    CompletionResponseFunctionCallSnapshot, CompletionResponseOutputSnapshot,
    CompletionResponseSnapshot, completion_response_message_text, completion_response_snapshot,
};

pub use tool_schema::programmatic_tool_declaration;

mod stable_schema;
pub(crate) use stable_schema::canonicalize_json;
pub use stable_schema::stable_tool_schemas;

mod summary;
pub use summary::summary_request;

mod estimate;
pub use estimate::estimate_text_input_tokens;
