use crate::request::TokenUsage;
use pl_protocol::TraceTextChannel;

/// Provider-independent streaming event consumed by the model accumulator.
#[derive(Debug, Clone)]
pub(crate) enum ModelStreamEvent {
    StepStarted {
        response_id: Option<String>,
    },
    TextStarted {
        id: String,
        channel: TraceTextChannel,
    },
    TextDelta {
        id: String,
        channel: TraceTextChannel,
        delta: String,
    },
    TextCompleted {
        id: String,
        channel: TraceTextChannel,
    },
    ReasoningStarted {
        id: String,
        provider_metadata: Option<serde_json::Value>,
    },
    ReasoningDelta {
        id: String,
        chunk_index: u32,
        delta: String,
    },
    ReasoningCompleted {
        id: String,
        provider_metadata: Option<serde_json::Value>,
    },
    PlanStarted {
        id: String,
    },
    PlanDelta {
        id: String,
        delta: String,
    },
    PlanCompleted {
        id: String,
    },
    ToolInputStarted {
        stream_id: Option<String>,
        item_id: String,
        call_id: Option<String>,
        name: Option<String>,
        payload_kind: ToolInputPayloadKind,
    },
    ToolInputDelta {
        stream_id: Option<String>,
        item_id: String,
        call_id: Option<String>,
        name: Option<String>,
        payload_delta: ToolInputDeltaPayload,
    },
    ToolInputCompleted {
        stream_id: Option<String>,
        item_id: String,
        call_id: Option<String>,
        name: Option<String>,
        payload: Option<ToolInputDeltaPayload>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolInputPayloadKind {
    FunctionArguments,
    CustomInput,
}

#[derive(Debug, Clone)]
pub(crate) enum ToolInputDeltaPayload {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolInputDeltaPayload {
    pub(crate) fn kind(&self) -> ToolInputPayloadKind {
        match self {
            Self::FunctionArguments(_) => ToolInputPayloadKind::FunctionArguments,
            Self::CustomInput(_) => ToolInputPayloadKind::CustomInput,
        }
    }

    pub(crate) fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(delta) | Self::CustomInput(delta) => delta,
        }
    }
}

impl ToolInputPayloadKind {
    pub(crate) fn empty_payload(self) -> ToolInputDeltaPayload {
        match self {
            Self::FunctionArguments => ToolInputDeltaPayload::FunctionArguments(String::new()),
            Self::CustomInput => ToolInputDeltaPayload::CustomInput(String::new()),
        }
    }
}
