use crate::WebSearchAction;
use crate::request::TokenUsage;
use pl_trace::TraceTextChannel;

/// Provider 无关的模型流式事件。
///
/// provider runtime 把私有 SSE chunk 转换为该事件流，调用方可以直接消费，
/// 也可以交给 `CompletionStreamAccumulator` 累计为 `CompletionResponse`。
#[derive(Debug, Clone)]
pub enum ModelStreamEvent {
    ResponseStarted {
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
    WebSearchStarted {
        item_id: String,
        action: WebSearchAction,
    },
    WebSearchCompleted {
        item_id: String,
        action: WebSearchAction,
        results: Option<Vec<serde_json::Value>>,
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
pub enum ModelBlockKind {
    Text {
        channel: TraceTextChannel,
    },
    ReasoningSummary,
    #[allow(dead_code)]
    Plan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelBlockField {
    Text,
    ReasoningSummary,
    #[allow(dead_code)]
    PlanContent,
}

#[derive(Debug, Clone)]
pub enum ModelBlockContent {
    Text(String),
    ReasoningSummary(Vec<String>),
    #[allow(dead_code)]
    Plan(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolInputPayloadKind {
    FunctionArguments,
    CustomInput,
}

#[derive(Debug, Clone)]
pub enum ToolInputDeltaPayload {
    FunctionArguments(String),
    CustomInput(String),
}

impl ToolInputDeltaPayload {
    pub fn kind(&self) -> ToolInputPayloadKind {
        match self {
            Self::FunctionArguments(_) => ToolInputPayloadKind::FunctionArguments,
            Self::CustomInput(_) => ToolInputPayloadKind::CustomInput,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Self::FunctionArguments(delta) | Self::CustomInput(delta) => delta,
        }
    }
}

impl ModelStreamEvent {
    pub fn text_started(id: String, channel: TraceTextChannel) -> Self {
        Self::BlockOpened {
            id,
            kind: ModelBlockKind::Text { channel },
            provider_metadata: None,
        }
    }

    pub fn text_delta(id: String, channel: TraceTextChannel, delta: String) -> Self {
        Self::BlockDelta {
            id,
            kind: ModelBlockKind::Text { channel },
            field: ModelBlockField::Text,
            delta,
            section_index: None,
        }
    }

    pub fn text_completed(
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

    pub fn reasoning_summary_started(
        id: String,
        provider_metadata: Option<serde_json::Value>,
    ) -> Self {
        Self::BlockOpened {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            provider_metadata,
        }
    }

    pub fn reasoning_summary_delta(id: String, section_index: u32, delta: String) -> Self {
        Self::BlockDelta {
            id,
            kind: ModelBlockKind::ReasoningSummary,
            field: ModelBlockField::ReasoningSummary,
            delta,
            section_index: Some(section_index),
        }
    }

    pub fn reasoning_summary_completed(
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
    pub fn empty_payload(self) -> ToolInputDeltaPayload {
        match self {
            Self::FunctionArguments => ToolInputDeltaPayload::FunctionArguments(String::new()),
            Self::CustomInput => ToolInputDeltaPayload::CustomInput(String::new()),
        }
    }
}
