//! Shared fixtures for the protocol-independent tool contract.
use pl_core::{
    context::OpaquePayload,
    tool::{
        ToolOutput,
        opaque::{CallContext, Tool, ToolError},
    },
};
pub(crate) fn input(value: serde_json::Value) -> OpaquePayload {
    OpaquePayload::new(
        "application/json",
        1,
        serde_json::to_string(&value).unwrap(),
    )
    .unwrap()
}
pub(crate) trait ToolTestExt: Tool {
    fn execute_raw(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> impl std::future::Future<Output = Result<ToolOutput, ToolError>> + Send {
        Tool::execute(self, input, context)
    }
}
impl<T: Tool> ToolTestExt for T {}

/// Minimal immutable context for protocol-independent tool fixtures.
pub(crate) fn thread_context() -> pl_core::tool::opaque::CallContext {
    pl_core::tool::opaque::CallContext {
        grant: Default::default(),
        context: Default::default(),
        model_projection: None,
        thread_id: "thread".into(),
        turn_id: "turn".into(),
        call_id: "call".into(),
        cancellation: tokio_util::sync::CancellationToken::new(),
        extensions: std::sync::Arc::new(Default::default()),
        extension_sequence: 0,
        catalog: Vec::new().into(),
        tasks: None,
    }
}

use crate::media::{
    ContextContent, ResourceReference, RetainedToolMedia, ToolMedia, ToolMediaHost,
};

use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Debug, Default)]
pub(crate) struct MemoryMedia(
    pub(crate) std::sync::Mutex<Vec<Arc<[u8]>>>,
    pub(crate) std::sync::Mutex<Vec<Option<Arc<[u8]>>>>,
);
impl ToolMediaHost for MemoryMedia {
    async fn retain(&self, media: ToolMedia) -> Result<RetainedToolMedia, ToolError> {
        let digest = format!("sha256:{:x}", Sha256::digest(&media.bytes));
        let reference = ResourceReference::new(
            digest.clone(),
            digest,
            media.bytes.len() as u64,
            media.media_type,
        )
        .map_err(ToolError::new)?;
        self.0.lock().unwrap().push(media.bytes);
        self.1
            .lock()
            .unwrap()
            .push(media.model_image.map(|image| image.bytes));
        Ok(RetainedToolMedia {
            context: vec![ContextContent::Resource {
                reference: reference.clone(),
            }],
            reference,
        })
    }
}
