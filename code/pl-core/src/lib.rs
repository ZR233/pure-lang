mod agent_runtime;
pub mod atomic_file;
pub mod attachment;
pub mod config;
mod context_assembler;
mod context_compaction;
mod core;
pub mod execution_environment;
pub mod instruction;
mod interaction;
pub mod mcp;
mod message;
mod model_config;
pub mod path_safety;
mod permission;
pub mod process;
mod prompt_cache;
pub mod remote;
pub mod runtime_usage;
pub mod session;
pub mod skill;
mod thread_event;
mod time;
pub mod tool;
mod trace;
pub mod turn;
mod web_search;
pub mod workflow;
mod working_set;
mod workspace;

pub use agent_runtime::*;
pub use attachment::{AttachmentRuntime, MaterializedAttachment, ToolImageAttachmentInput};
pub use config::{
    BuiltinMcpServerState, DEFAULT_PROJECT_DOC_MAX_BYTES, EffectiveMcpServerConfig,
    InstructionsConfig, McpServerConfig, McpServerMutationPolicy, McpServerSourceKind,
    McpServerStatusKind, McpServerTransport, ReasoningEffort, RuntimeConfig, SkillsConfig,
    SystemSkillsConfig, ToolCapabilityConfig, active_mcp_server_names, builtin_mcp_server_ids,
    effective_mcp_servers, is_builtin_mcp_server_id, normalize_builtin_mcp_server_states,
    validate_builtin_mcp_server_states, validate_mcp_servers, zhipu_coding_plan_token,
};
pub use context_assembler::{AssembledModelContext, ContextAssembler, TurnContextSnapshot};
pub use context_compaction::{
    ContextCompactionConfig, ContextCompactionImplementation, ContextCompactionPhase,
    ContextCompactionReplacement, ContextCompactionSnapshot, ContextCompactionTrigger,
    ManualContextCompactionRequest, RecentInteractionTailConfig,
};
pub use core::{
    BuiltinToolInstaller, CoreRuntimeProfile, ModelTurnClient, ModelTurnOptions, ModelTurnRequest,
    ToolProfile, TurnEngine, TurnEngineBuilder,
};
pub use execution_environment::{
    ExecutionEnvironment, ExecutionOs, ExecutionTransport, ShellDialect, resolve_local_shell,
};
pub use interaction::{
    UserInputOptionProjection, UserInputProjection, UserInputQuestionProjection,
    project_user_input_questions,
};
pub use mcp::{
    ConnectedMcp, McpAvailabilityKind, McpAvailabilitySnapshot, McpConnectRequest, McpConnector,
    McpGeneration, McpImageOutputContext, McpResetScope, McpRuntime, McpRuntimeHandle,
    McpRuntimeToolDescriptor, McpTurnLease,
};
pub use message::{
    CompletionResponseOutputSnapshot, CompletionResponseSnapshot, append_message_fragment_text,
    assistant_reasoning_message, assistant_text_message, completion_response_message_text,
    completion_response_snapshot, is_compaction_summary_text, message_content_text,
    text_preview_chars, user_message_text, user_text_message,
};
pub use model_config::{
    AgentModelConfig, AgentRoleId, ModelCatalog, ModelCatalogId, ModelRouteConfig,
    ProviderCapabilitySelection, ProviderCatalogRegistry, ProviderConfig, ProviderId,
    ProviderModelCatalogConfig, ProviderPreset, ProviderPresetId, ResolvedModelRoute,
    builtin_model_catalog, builtin_provider_catalog, provider_service_capabilities_descriptor,
};
pub use pl_lsp::{
    LspActivityKind, LspAvailabilityKind, LspDiagnostic, LspPosition, LspQuery, LspQueryOperation,
    LspQueryResult, LspRange, LspRuntimeRegistry, LspServerSnapshot,
};
pub use pl_model::{
    MediaMixPolicy, MediaRepresentation, ModelCapabilities, ModelInfo, ModelInputCapability,
    ModelInputLimits, ModelInputSource, ModelMediaInputProfile, ModelModality, ModelParameter,
    ModelRequestProfile, OpenAiCompactionMode, PromptCacheDialect, PromptCacheProviderCapabilities,
    ProviderConnectionMode, ProviderEndpoint, ProviderServiceCapabilities, ProviderWireProtocol,
    ReasoningConfig, ReasoningInterleaved, ReasoningInterleavedField, ReasoningSummary,
    ResponsesHostedToolCapabilities, StandaloneWebSearchDialect, ToolCapabilities, ToolSpec,
    ToolWirePolicy, TruncationMode, WebSearchConfig, WebSearchContextSize, WebSearchFilters,
    WebSearchMode, WebSearchProviderCapabilities, WebSearchUserLocation,
    deepseek_default_model_slugs, default_models, mimo_default_model_slugs,
    openai_default_model_slugs, zhipu_default_model_slugs,
};
pub use pl_protocol::{
    AgentRuntimeDelta, AttachmentModality, BudgetLimitKind, BudgetUsage, ContentPart,
    ContextSectionId, ErrorSeverity, InteractionChangedEvent, InteractionContent, InteractionKind,
    InteractionRequest, InteractionResolution, InteractionScope, InteractionStatus,
    McpAvailabilityDescriptor, McpHealthSnapshot, McpServerDescriptor, Message, MessageContent,
    MessageRole, ModelContextItem, OutputStream, PermissionLevel, PinnedContextSection,
    PipelineStage, ProviderCatalogSnapshot, ProviderConnectionModeDescriptor,
    ProviderPresetDescriptor, ProviderServiceCapabilitiesDescriptor, PureError, Result,
    RetryDisposition, RuntimeCostAmount, RuntimeUsageSnapshot, SkillActivation, TokenUsageSnapshot,
    ToolApprovalResolution, ToolResultReceipt, TurnFailure, TurnFailureCategory, UserInputAnswer,
    UserInputRequest, UserInputResponse, UserQuestion, UserQuestionOption,
};
pub(crate) use prompt_cache::{
    PromptCacheInput, derive_prompt_cache_key, prepare_prompt_context, stable_tool_schemas,
};
pub use runtime_usage::ModelTokenUsageSnapshot;
pub use session::{AgentSession, AgentSessionForkPolicy};
pub use skill::{SkillCatalog, SkillMetadata, SkillSourceKind};
pub use thread_event::{
    ThreadEventBus, ThreadEventBusHandle, ThreadEventError, ThreadEventOptions,
    ThreadEventSubscription, ThreadHotHistory, ThreadNotificationFact,
};
pub use tool::*;
pub use trace::TraceRecorder;
pub use turn::{
    AGENT_MAX_COUNT, AGENT_MAX_DEPTH, DEFAULT_WALL_CLOCK_MS, InteractionCallback,
    InteractionFuture, PermissionMode, ToolApprovalDecision, ToolApprovalRequest, ToolCompletion,
    ToolCompletionCallback, ToolCompletionFuture, ToolEffect, ToolExecutionMode, TurnBudget,
    TurnOptions, TurnRequest, TurnResult, UserInputMode,
};
pub use web_search::{
    ToolVisibilityConstraint, WebSearchAvailability, WebSearchBackend, WebSearchBackendKind,
    WebSearchPath, WebSearchPlan, WebSearchPlans, WebSearchResolution, plan_web_search,
    plan_web_searches,
};
pub use working_set::{
    CONVERSATION_RECOVERY_SECTION_ID, CURRENT_TODO_SECTION_ID, EVIDENCE_LEDGER_SECTION_ID,
    MAX_PINNED_CONTEXT_BYTES, MAX_PINNED_SECTION_BYTES, MAX_SESSION_NOTE_BYTES,
    TurnWorkingSetChange, TurnWorkingSetHandle, WORKFLOW_CONTEXT_SECTION_ID,
    canonical_content_hash, canonical_json_hash, context_section,
};
pub use workspace::{
    WorkspaceInstructionDocument, WorkspaceInstructions, load_workspace_instruction_documents,
    resolve_workspace_root,
};
