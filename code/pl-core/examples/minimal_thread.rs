//! Run with `cargo run -p pl-core --example minimal_thread --no-default-features`.
//! A custom model and tool exchange arbitrary strings without a provider protocol or database.
use pl_core::context::{ContextContent, ContextSource, OpaquePayload};
use pl_core::model::{
    DynModelSession, ModelError, ModelRequest, ModelSession, ModelStepOutput, ModelToolCall,
    PreparedModelCall,
};
use pl_core::thread::{ThreadHandle, TurnInput, journal};
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, Tool, ToolError},
};
use std::{num::NonZeroU32, sync::Arc};
use tokio_util::sync::CancellationToken;

struct DemonstrationModel;
impl ModelSession for DemonstrationModel {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        let answered = request
            .context
            .records
            .iter()
            .any(|record| matches!(record.source, ContextSource::ToolResult { .. }));
        let output = ModelStepOutput {
            attempt_id: request.attempt_id,
            base_context_revision: request.context.revision,
            content: if answered {
                vec![ContextContent::Text {
                    text: Arc::from("The tool result is now part of the committed context."),
                }]
            } else {
                Vec::new()
            },
            tool_calls: if answered {
                Vec::new()
            } else {
                vec![ModelToolCall {
                    call_id: "echo-call".into(),
                    tool_id: "echo".into(),
                    arguments: OpaquePayload::text("  arbitrary syntax: 你好\n"),
                }]
            },
            private_context: None,
            usage: Default::default(), // Unknown service usage remains unknown.
        };
        Ok(PreparedModelCall::new(async move { Ok(output) }))
    }
    async fn close(&mut self) -> Result<(), ModelError> {
        Ok(())
    }
}

#[derive(Debug)]
struct Echo;
impl Tool for Echo {
    async fn execute(&self, input: OpaquePayload, _: CallContext) -> Result<ToolOutput, ToolError> {
        let context = vec![ContextContent::Text {
            text: Arc::from(input.content()),
        }];
        Ok(ToolOutput::new(input, context))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let thread = ThreadHandle::start("example".into(), DynModelSession::new(DemonstrationModel))?;
    thread
        .register_tools(vec![Registration::new(
            "echo".into(),
            OpaquePayload::new("example.tool-declaration", 1, "Echo arbitrary UTF-8 text")?,
            Echo,
        )?])
        .await?;
    let completed = thread
        .run_turn(TurnInput {
            turn_id: "turn-1".into(),
            attempt_prefix: "attempt-1".into(),
            content: vec![ContextContent::Text {
                text: Arc::from("Please call echo."),
            }],
            max_model_steps: NonZeroU32::new(4).ok_or("invalid step limit")?,
            cancellation: CancellationToken::new(),
        })
        .await?;
    let commits = thread.journal().await?;
    let encoded = commits
        .iter()
        .map(|commit| commit.encode())
        .collect::<Result<Vec<_>, _>>()?;
    let decoded = encoded
        .iter()
        .map(|payload| journal::ThreadCommit::decode(payload).map(Arc::new))
        .collect::<Result<Vec<_>, _>>()?;
    let replayed = journal::replay(&decoded)?;
    assert_eq!(replayed.context, thread.snapshot().context);
    println!(
        "{} model steps, {} tool deliveries, {} commits",
        completed.model_steps,
        replayed.deliveries.len(),
        replayed.commit_sequence
    );
    thread.close().await?;
    Ok(())
}
