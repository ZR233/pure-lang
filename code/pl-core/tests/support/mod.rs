use std::sync::{Arc, Mutex};

use pl_core::context::{ContextContent, ContextSnapshot, ContextSource, OpaquePayload};
use pl_core::model::{
    ModelError, ModelRequest, ModelSession, ModelStepOutput, ModelToolCall, PreparedModelCall,
};
use pl_core::thread::{ModelStepLimit, TurnInput};
use tokio_util::sync::CancellationToken;

pub fn text(value: &str) -> ContextContent {
    ContextContent::Text {
        text: Arc::from(value),
    }
}

pub fn turn(id: &str) -> TurnInput {
    TurnInput {
        turn_id: id.into(),
        attempt_prefix: format!("attempt-{id}"),
        content: vec![text("Please handle this request")],
        max_model_steps: ModelStepLimit::Limited(3.try_into().expect("nonzero limit")),
        cancellation: CancellationToken::new(),
    }
}

pub fn response(request: &ModelRequest, tool_calls: Vec<ModelToolCall>) -> ModelStepOutput {
    ModelStepOutput {
        attempt_id: request.attempt_id.clone(),
        base_context_revision: request.context.revision,
        content: vec![text(if tool_calls.is_empty() {
            "Answer completed"
        } else {
            "Calling tools"
        })],
        tool_calls,
        private_context: None,
        usage: Default::default(),
    }
}

pub struct ScriptedModel {
    pub tool_ids: Vec<String>,
    pub requests: Arc<Mutex<Vec<ContextSnapshot>>>,
}

impl ScriptedModel {
    pub fn new(tool_ids: &[&str]) -> (Self, Arc<Mutex<Vec<ContextSnapshot>>>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                tool_ids: tool_ids.iter().map(|id| (*id).into()).collect(),
                requests: requests.clone(),
            },
            requests,
        )
    }
}

impl ModelSession for ScriptedModel {
    async fn prepare(&mut self, request: ModelRequest) -> Result<PreparedModelCall, ModelError> {
        self.requests
            .lock()
            .expect("request log")
            .push(request.context.clone());
        let has_result = request.context.records.iter().any(|record| {
            matches!(&record.source, ContextSource::ToolResult { .. })
                && record.turn_id.as_deref() == Some(request.turn_id.as_str())
        });
        let calls = if has_result {
            Vec::new()
        } else {
            self.tool_ids
                .iter()
                .map(|tool_id| ModelToolCall {
                    call_id: format!("{}-{tool_id}", request.turn_id),
                    tool_id: tool_id.clone(),
                    arguments: OpaquePayload::text(format!("  raw {tool_id}\n")),
                })
                .collect()
        };
        Ok(PreparedModelCall::new(async move {
            Ok(response(&request, calls))
        }))
    }

    async fn close(&mut self) -> Result<(), ModelError> {
        Ok(())
    }
}
