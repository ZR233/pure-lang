//! A single completion facade backed by concrete, accessible provider clients.
mod client;
mod clock;
mod compaction;
mod context;
mod host_call;
mod hosted;
pub use hosted::{HostedTool, HostedWebSearchOptions};
mod invocation;
pub(crate) mod openai;
mod provider_error;
mod responses_websocket;
mod session;
mod summary;
pub use summary::{TextSummary, TextSummaryRequest};
pub(crate) mod transport_policy;
pub(crate) mod wire_capture;

pub use client::{ModelRuntime, NativeCompactionCheckpoint, RemoteCompaction};
pub use clock::InferenceClock;
pub(crate) use invocation::InvocationRunner;
pub use invocation::ModelInvocationContext;
#[cfg(test)]
pub(crate) use invocation::test_support;
pub(crate) use provider_error::provider_stream_failure;
pub use session::ModelSession;

pub use pl_protocol::trace::{AgentEvent, AgentEventSender, TraceEventSink};
pub use tokio_util::sync::CancellationToken;

pub use host_call::{ModelTurnClient, ModelTurnOptions, ModelTurnRequest};

mod cache_key;
pub use cache_key::{binding_cache_namespace, derive_prompt_cache_key};

mod prompt_diagnostics;
pub use prompt_diagnostics::{PromptCacheInput, prompt_diagnostics};

pub use pl_protocol::{PromptPrefixChangedReason, ThreadPromptMetadata, ThreadPromptSnapshot};

mod thread_model;
pub use thread_model::{
    ModelCallBinding, ModelFailureReceipt, ModelRequestReceipt, ModelResponseReceipt,
    ThreadCompaction, ThreadCompactionOptions, ThreadCompactionStrategy, ThreadModel,
    attachment_content, model_failure_receipt, model_request_receipt, model_response_receipt,
    thread_tool_declaration,
};
