//! 单个 language server 的连接、文档同步与 JSON-RPC 边界。

mod configuration;
mod connection;
mod diagnostics;
mod documents;
mod lifecycle;
mod message;
mod retry;
mod rpc;
mod status;
mod transport;
pub(crate) mod uri;

pub(crate) use connection::LspClient;
pub(crate) use diagnostics::DiagnosticSink;
pub(crate) use retry::with_content_modified_retries;
pub(crate) use status::LspClientRuntimeStatus;
