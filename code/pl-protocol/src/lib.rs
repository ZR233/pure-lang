mod accounting;
mod activity;
mod agent;
mod agent_profile;
pub mod agent_session;
mod billing;
mod chat_window;
mod error;
mod event;
mod id;
mod interaction;
mod labeled;
mod mcp;
mod message;
mod model_context;
mod observed_state;
mod permission;
pub mod process_worker;
mod provider_catalog;
pub mod remote;
pub mod session_runtime;
pub mod studio;
pub mod thread;
mod thread_item;
mod tool;
mod turn;
mod turn_failure;
mod workflow;

pub use accounting::{
    InferenceAccounting, PricingMode, PricingOutcome, UnpricedReason, UsageReport, UsageStatus,
};
pub use activity::{
    ACTIVITY_SUMMARY_LIMIT, ThreadActivity, ThreadActivityArguments, ThreadActivityContentPart,
    ThreadActivityDetail, ThreadActivityDetailQuery, ThreadActivityKind, ThreadActivityToolDetail,
    ThreadActivityToolEntry, ThreadActivityToolState, ThreadActivityTools,
};
pub use agent::*;
pub use agent_profile::{
    AgentProfileSnapshot, AgentWorkspaceAssignmentSnapshot, AgentWorkspaceDisposition,
    AgentWorkspaceMode, AgentWorktreeSnapshot,
};
pub use agent_session::plan::{
    AGENT_SESSION_PLAN_CONFIRMATION_QUESTION_ID, AgentSessionPlanAvailableTransition,
    AgentSessionPlanConfirmationPurpose, AgentSessionPlanDocument,
    AgentSessionPlanMutationResponse, AgentSessionPlanOperation, AgentSessionPlanOperationReceipt,
    AgentSessionPlanPhase, AgentSessionPlanResultCode, AgentSessionPlanSnapshot,
    AgentSessionPlanState, AgentSessionPlanTransitionActor, AgentSessionPlanTransitionError,
    AgentSessionPlanTransitionRecord,
};
pub use billing::{
    InferenceBillingAppend, InferenceBillingRecord, InferenceModelObservation,
    InferenceOrchestrationMetrics, InferenceTiming, InferenceTokenUsage, ModelMatchState,
    ModelPricingSnapshot, TurnBillingRecord,
};
pub use chat_window::{
    ChatWindowChange, ChatWindowDirection, ChatWindowFocus, ChatWindowItem, ChatWindowLifecycle,
    ChatWindowPriority, ChatWindowQuery, ChatWindowSnapshot, ChatWindowUpdate, ThreadContentField,
    ThreadFieldChange, ThreadFieldUpdate,
};
pub use error::{PureError, Result};
pub use event::{
    AgentRuntimeDelta, BudgetLimitKind, BudgetLimitSnapshot, BudgetUsage, ErrorSeverity,
    OutputStream, PipelineStage, RuntimeCostAmount, RuntimeUsageSnapshot, SkillActivation,
    SkillActivationCause, SkillActivationResourceBase, TodoItem, TodoListSnapshot, TodoStatus,
    TokenUsageSnapshot, UserInputAnswer, UserInputRequest, UserInputResponse, UserQuestion,
    UserQuestionOption,
};
pub use id::{AgentRoleId, ThreadId, TurnId};
pub use interaction::*;
pub use labeled::{LabeledEnum, UnknownLabelError};
pub use mcp::{McpAvailabilityDescriptor, McpHealthSnapshot, McpServerDescriptor};
pub use message::{
    AttachmentModality, ContentPart, Message, MessageContent, MessagePresentation, MessageRole,
    ToolCallCaller, ToolCallKind, ToolCallRecord, ToolResultRecord,
};
pub use model_context::{
    AgentSessionSnapshot, AgentWorkingState, ContextSectionId, ContextSectionIdError,
    ConversationExternalStatePolicy, ConversationRecoveryMode, ConversationRecoveryRecord,
    ConversationRecoveryState, ConversationRecoveryTurnRange, ModelContextItem,
    ModelContextSectionSnapshot, ModelContextSnapshot, PinnedContextSection,
    PromptPrefixChangedReason, ResponsesContextItem, ResponsesContextItemKind, SessionNote,
    ThreadPromptMetadata, ThreadPromptSnapshot, ToolDiscoveryState, ToolMediaContext,
    ToolResultReceipt,
};
pub use observed_state::{
    DegradedResource, FailedResource, LoadingResource, ObservedResource, ObservedResourceCommand,
    ObservedResourceKind, ObservedResourceTransitionDecision, ObservedResourceTransitionError,
    ReadyResource, RefreshingResource, StaleResource, StateError, StateOperation, StoppedResource,
    UninitializedResource,
};
pub use permission::PermissionLevel;
pub use provider_catalog::{
    CredentialDescriptorDto, ModelCapabilitiesDto, ModelCatalogDescriptor, ModelDescriptor,
    ModelInputCapabilityDto, ModelInputSourceDto, ModelModalityDto, ModelPriceTierDto,
    ModelPricingDto, ModelReasoningDescriptor, ModelTransportDescriptor,
    PROVIDER_CATALOG_SCHEMA_VERSION, ProviderCatalogSnapshot, ProviderConnectionModeDescriptor,
    ProviderPresetDescriptor, ProviderServiceCapabilitiesDescriptor,
    WebSearchProviderCapabilitiesDescriptor, WebSearchResolutionDescriptor,
};
// 持久化观测契约（队列压力、逐 Thread 水位）同时由 runtime 协调器与其上层
// bridge/HTTP 适配器命名，因此在 crate 根重导出，消费方无需再拼 `studio::` 路径。
pub use studio::{
    PersistenceQueueSnapshot, ThreadPersistenceSnapshot, ThreadStorageExecution, ThreadStorageState,
};
pub use thread::mode::{ThreadModeCatalogSnapshot, ThreadModeDescriptor, ThreadModeId};
pub use thread::{
    CacheUsageSummary, THREAD_SCHEMA_VERSION, Thread, ThreadContextDisposition,
    ThreadModelRouteSnapshot, ThreadNotification, ThreadNotificationEnvelope,
    ThreadRuntimeSnapshot, ThreadRuntimeUsage, ThreadSnapshot, ThreadStatus,
    ThreadSubscriptionRequest, ThreadSubscriptionUpdate, ThreadTurnHistory, ThreadTurnPage,
    ThreadWorkspaceMode, TimelineItemQuery, TimelineItemRead, TimelinePage, TimelineQuery,
    TimelineTurn,
};
mod session_entry;
pub use session_entry::SessionEntry;
pub use thread_item::*;
pub use tool::{
    HostedWebSearchDialect, HostedWebSearchOptions, ToolCallerMode, ToolFormat, ToolSpec,
    WebSearchContextSize, WebSearchFilters, WebSearchUserLocation, WebSearchUserLocationType,
};
pub use turn::{
    BudgetLimitedTurnOutcome, BudgetLimitedTurnState, CancelledTurnOutcome, CancelledTurnState,
    CompletedTurnOutcome, CompletedTurnState, FailedTurnOutcome, FailedTurnState, QueuedTurnState,
    RunningTurnState, Turn, TurnCancellationCause, TurnCommand, TurnCompletion, TurnOutcome,
    TurnPhase, TurnRolloverOutcome, TurnState, TurnTransitionDecision, TurnTransitionError,
};
pub use turn_failure::{
    ProviderFailure, ProviderFailureContext, ProviderFailureKind, ProviderFailureStage,
    ProviderRecovery, RetryDisposition, TurnFailure, TurnFailureCategory,
};
pub use workflow::{
    WorkflowDefinition, WorkflowOperationReceipt, WorkflowRun, WorkflowRunArchive,
    WorkflowRunLifecycle, WorkflowRuntimeRunSnapshot, WorkflowRuntimeSnapshot,
    WorkflowSessionState, WorkflowState, WorkflowStateKind, WorkflowTransition,
    WorkflowTransitionRecord,
};

/// Search request and configuration interchange shared by upper adapters.
pub mod search;

/// Product trace records and their typed wire projection.
pub mod trace;

/// Shared model/tool projection materials; core carries their encoded payload without decoding.
pub mod tool_projection;
