mod error;
mod event;
mod interaction;
mod mcp;
mod message;
mod model_context;
mod permission;
mod provider_catalog;
mod session;
#[cfg(feature = "typescript")]
mod typescript;

pub use error::{PureError, Result};
pub use event::{
    AgentRuntimeDelta, AgentStatus, BudgetLimitKind, BudgetUsage, ErrorSeverity, OutputStream,
    PipelineStage, PlanLifecycleEvent, PlanLifecycleState, RuntimeCostAmount, RuntimeUsageSnapshot,
    SkillActivation, SubAgentActivityKind, TodoItem, TodoListSnapshot, TodoStatus,
    TokenUsageSnapshot, UserInputAnswer, UserInputRequest, UserInputResponse, UserQuestion,
    UserQuestionOption,
};
pub use interaction::{
    InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
    InteractionResolution, InteractionScope, InteractionStatus, PlanConfirmationResolution,
    ToolApprovalResolution,
};
pub use mcp::{McpAvailabilityDescriptor, McpHealthSnapshot, McpServerDescriptor};
pub use message::{
    ContentPart, ImageSource, Message, MessageContent, MessageRole,
    TOOL_CALL_ARGUMENTS_METADATA_KEY, TOOL_CALL_CALL_ID_METADATA_KEY, TOOL_CALL_ID_METADATA_KEY,
    TOOL_CALL_KIND_METADATA_KEY, TOOL_CALLS_METADATA_KEY, TOOL_NAME_METADATA_KEY,
    ToolCallHistoryMetadata, ToolCallKind, ToolResultMetadata,
};
pub use model_context::{
    ContextSectionId, ContextSectionIdError, ModelContextItem, PinnedContextSection,
    ToolResultReceipt,
};
pub use permission::PermissionLevel;
pub use provider_catalog::{
    CredentialDescriptorDto, ModelCapabilitiesDto, ModelCatalogDescriptor, ModelDescriptor,
    ModelPricingDto, ModelReasoningDescriptor, PROVIDER_CATALOG_SCHEMA_VERSION,
    ProviderCatalogSnapshot, ProviderConnectionModeDescriptor, ProviderPresetDescriptor,
    ProviderServiceCapabilitiesDescriptor, ProviderTransportDescriptor,
    WebSearchProviderCapabilitiesDescriptor, WebSearchResolutionDescriptor,
};
pub use session::{
    SESSION_EVENT_SCHEMA_VERSION, SessionAgentPart, SessionAgentSnapshot, SessionAttachment,
    SessionContextCompaction, SessionEventEnvelope, SessionEventKind, SessionEventPosition,
    SessionMessage, SessionMessageRole, SessionMessageStatus, SessionPart, SessionPartContent,
    SessionPartDelta, SessionPartDeltaField, SessionPartStatus, SessionResyncReason,
    SessionRuntimeSnapshot, SessionRuntimeUsage, SessionStreamFrame, SessionSubscriptionRequest,
    SessionTextChannel, SessionTimelineEvent, SessionTimelineEventKind, SessionToolPart,
    SessionTurn, SessionTurnStatus, SessionViewSnapshot,
};
#[cfg(feature = "typescript")]
pub use typescript::session_events_typescript;
