use pl_protocol::TimelineTextChannel;

use crate::request::TokenUsage;

/// Provider-independent streaming event consumed by the model accumulator.
#[derive(Debug, Clone)]
pub(crate) enum ModelStreamEvent {
    StepStarted {
        response_id: Option<String>,
    },
    TextDelta {
        item_id: Option<String>,
        channel: Option<TimelineTextChannel>,
        delta: String,
    },
    ReasoningDelta {
        item_id: Option<String>,
        chunk_index: u32,
        delta: String,
    },
    ToolInputDelta {
        stream_id: Option<String>,
        item_id: String,
        call_id: Option<String>,
        name: Option<String>,
        payload_delta: ToolInputDeltaPayload,
    },
    ToolCallReady {
        stream_id: Option<String>,
        item_id: String,
        call_id: Option<String>,
        name: Option<String>,
        payload: Option<ToolInputDeltaPayload>,
    },
    Usage(TokenUsage),
    Completed {
        response_id: Option<String>,
    },
    Failed {
        code: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum ToolInputDeltaPayload {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolInputDeltaPayload {
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(delta) | Self::CustomInput(delta) => delta,
        }
    }
}
