//! A single completion facade backed by concrete, accessible provider clients.
mod client;
mod clock;
mod compaction;
mod invocation;
pub(crate) mod openai;
mod provider_error;
mod responses_websocket;
mod session;
pub(crate) mod transport_policy;
pub(crate) mod wire_capture;

pub use client::{ModelRuntime, RemoteCompaction};
pub use clock::InferenceClock;
pub(crate) use invocation::InvocationRunner;
pub use invocation::ModelInvocationContext;
#[cfg(test)]
pub(crate) use invocation::test_support;
pub(crate) use provider_error::provider_stream_failure;
pub use session::ModelSession;

pub use pl_trace::{AgentEvent, AgentEventSender, TraceEventSink};
pub use tokio_util::sync::CancellationToken;
