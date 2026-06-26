use crate::request::TokenUsage;
use pl_trace::TraceTextChannel;

/// Provider-independent streaming event consumed by the model accumulator.
#[derive(Debug, Clone)]
pub(crate) enum ModelStreamEvent {
    StepStarted {
        response_id: Option<String>,
    },
    BlockOpened {
        id: String,
        kind: ModelBlockKind,
        provider_metadata: Option<serde_json::Value>,
    },
    BlockDelta {
        id: String,
        kind: ModelBlockKind,
        field: ModelBlockField,
        delta: String,
        section_index: Option<u32>,
    },
    BlockClosed {
        id: String,
        kind: ModelBlockKind,
        authoritative_content: Option<ModelBlockContent>,
        provider_metadata: Option<serde_json::Value>,
    },
    ReasoningRawDelta {
        id: String,
        content_index: u32,
        delta: String,
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
pub(crate) enum ModelBlockKind {
    Text {
        channel: TraceTextChannel,
    },
    ReasoningSummary,
    #[allow(dead_code)]
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ModelBlockField {
    Text,
    ReasoningSummary,
    #[allow(dead_code)]
    PlanContent,
}

#[derive(Debug, Clone)]
pub(crate) enum ModelBlockContent {
    Text(String),
    ReasoningSummary(Vec<String>),
    #[allow(dead_code)]
    Plan(String),
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

impl ModelStreamEvent {
    pub(crate) fn text_started(id: String, channel: TraceTextChannel) -> Self {
        Self::BlockOpened {
            id,
            kind: ModelBlockKind::Text { channel },
            provider_metadata: None,
        }
    }

    pub(crate) fn text_delta(id: String, channel: TraceTextChannel, delta: String) -> Self {
        Self::BlockDelta {
            id,
            kind: ModelBlockKind::Text { channel },
            field: ModelBlockField::Text,
            delta,
            section_index: None,
        }
    }

    pub(crate) fn text_completed(
        id: String,
        channel: TraceTextChannel,
        authoritative_text: Option<String>,
    ) -> Self {
        Self::BlockClosed {
            id,
            kind: ModelBlockKind::Text { channel },
            authoritative_content: authoritative_text.map(ModelBlockContent::Text),
            provider_metadata: None,
        }
    }

    pub(crate) fn reasoning_summary_started(
        id: String,
        provider_metadata: Option<serde_json::Value>,
    ) -> Self {
        Self::BlockOpened {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            provider_metadata,
        }
    }

    pub(crate) fn reasoning_summary_delta(id: String, section_index: u32, delta: String) -> Self {
        Self::BlockDelta {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            field: ModelBlockField::ReasoningSummary,
            delta,
            section_index: Some(section_index),
        }
    }

    pub(crate) fn reasoning_summary_completed(
        id: String,
        provider_metadata: Option<serde_json::Value>,
        authoritative_summary: Option<Vec<String>>,
    ) -> Self {
        Self::BlockClosed {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            authoritative_content: authoritative_summary.map(ModelBlockContent::ReasoningSummary),
            provider_metadata,
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
