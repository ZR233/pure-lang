use std::sync::{Arc, Mutex};

use pl_core::context::OpaquePayload;
use pl_core::tool::{
    ToolOutput,
    opaque::{CallContext, Registration, Tool, ToolError},
};

#[derive(Debug)]
struct RecordingTool {
    log: Arc<Mutex<Vec<String>>>,
}

impl Tool for RecordingTool {
    async fn execute(
        &self,
        input: OpaquePayload,
        context: CallContext,
    ) -> Result<ToolOutput, ToolError> {
        self.log.lock().expect("tool log").push(context.call_id);
        Ok(ToolOutput::new(
            input.clone(),
            vec![crate::support::text(input.content())],
        ))
    }
}

pub fn tool(id: &str, log: &Arc<Mutex<Vec<String>>>) -> Registration {
    Registration::new(
        id.into(),
        OpaquePayload::text(format!("Tool {id}")),
        RecordingTool { log: log.clone() },
    )
    .expect("valid tool identity")
    .foreground_coexisting()
}
