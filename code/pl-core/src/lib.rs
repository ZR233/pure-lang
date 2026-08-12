mod agent_runtime;
pub mod attachment;
pub mod config;
mod context_assembler;
mod context_compaction;
mod core;
mod instruction;
mod interaction;
pub mod mcp;
mod message;
mod model_config;
pub mod path_safety;
mod permission;
mod process;
mod prompt_cache;
pub mod runtime_usage;
mod session;
pub mod skill;
mod thread_event;
pub mod tool;
mod trace;
pub mod turn;
mod web_search;
mod working_set;
mod workspace;

pub use agent_runtime::*;
pub use attachment::MaterializedAttachment;
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
    CoreModelTurnClient, CoreModelTurnOptions, CoreModelTurnRequest, CoreRuntimeOptions,
    CoreRuntimeProfile, SharedToolSchemaOptions, ToolProfile, ToolSetBuilder, ToolVisibilitySet,
    TurnEngine, TurnEngineBuilder, WorkspaceProfile, shared_tool_names, shared_tool_schemas,
    stream_history_completion_message_text, stream_session_completion_message_text,
    stream_session_completion_response,
};
pub use instruction::{
    ExecutionInstructionProfile, InstructionAssembler, InstructionAssemblyRequest,
    InstructionBlock, InstructionBundle, InstructionProfile, InstructionSnapshot,
    InstructionSource, InstructionSourceKind,
};
pub use interaction::{
    UserInputOptionProjection, UserInputProjection, UserInputQuestionProjection,
    project_user_input_questions,
};
pub use mcp::{
    ConnectedMcp, McpAvailabilityKind, McpAvailabilitySnapshot, McpConnectRequest, McpConnector,
    McpGeneration, McpRuntime, McpRuntimeHandle, McpRuntimeToolDescriptor, McpTurnLease,
};
pub use message::{
    CompletionResponseOutputSnapshot, CompletionResponseSnapshot, append_message_fragment_text,
    assistant_reasoning_message, assistant_text_message, completion_response_message_text,
    completion_response_preview, completion_response_snapshot, is_compaction_summary_text,
    message_content_text, message_content_text_lines, text_preview_chars, user_message_text,
    user_text_message,
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
    DeepSeekBalanceInfo, DeepSeekBalanceUsage, ModelCapabilities, ModelInfo, ModelModality,
    ModelParameter, ModelRequestProfile, OpenAiCompactionMode, PromptCacheDialect,
    PromptCacheProviderCapabilities, ProviderConnectionMode, ProviderServiceCapabilities,
    ProviderWireProtocol, ReasoningInterleaved, ReasoningInterleavedField,
    ResponsesHostedToolCapabilities, StandaloneWebSearchDialect, ToolCapabilities, ToolWirePolicy,
    TruncationMode, WebSearchProviderCapabilities, ZhipuCodingPlanUsage, ZhipuQuotaLimit,
    ZhipuQuotaWindow, ZhipuToolUsageDetail,
};
pub use pl_protocol::{
    AgentRuntimeDelta, BudgetLimitKind, BudgetUsage, ContentPart, ContextSectionId, ErrorSeverity,
    ImageSource, InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
    InteractionResolution, InteractionScope, InteractionStatus, McpAvailabilityDescriptor,
    McpHealthSnapshot, McpServerDescriptor, Message, MessageContent, MessageRole, ModelContextItem,
    OutputStream, PermissionLevel, PinnedContextSection, PipelineStage, PlanConfirmationResolution,
    ProviderCatalogSnapshot, ProviderConnectionModeDescriptor, ProviderPresetDescriptor,
    ProviderServiceCapabilitiesDescriptor, PureError, Result, RetryDisposition, RuntimeCostAmount,
    RuntimeUsageSnapshot, SkillActivation, TokenUsageSnapshot, ToolApprovalResolution,
    ToolResultReceipt, TurnFailure, TurnFailureCategory, UserInputAnswer, UserInputRequest,
    UserInputResponse, UserQuestion, UserQuestionOption,
};
pub(crate) use prompt_cache::{
    PromptCacheInput, derive_prompt_cache_key, prepare_prompt_context, stable_tool_schemas,
};
pub use runtime_usage::ModelTokenUsageSnapshot;
pub use session::{
    AgentSession, AgentSessionForkPolicy, repair_incomplete_tool_history,
    tool_call_history_message, tool_result_history_message,
};
pub use skill::{SkillCatalog, SkillMetadata, SkillSourceKind};
pub use thread_event::{
    ThreadEventBus, ThreadEventBusHandle, ThreadEventError, ThreadEventOptions,
    ThreadEventSubscription, ThreadNotificationFact,
};
#[cfg(feature = "docker-tools")]
pub use tool::DockerCliContainerBackend;
pub use tool::{
    AgentWorkspace, AskUserTool, CommandBackend, CommandOutputObserver, CommandOutputSizes,
    CommandOutputSnapshot, CommandOutputStream, CommandOutputTarget, CommandProcessManager,
    CommandSpawnRequest, CommandStartRequest, CommandWriteRequest, ContainerBackend,
    ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecOutput, ContainerExecRequest,
    ContainerWorkspaceFileBackend, DEFAULT_MODEL_TOOL_OUTPUT_BATCH_TOKENS,
    DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, ExecInput, ExecTool, ExecutionBackend, ExecutionOutput,
    ExecutionRequest, GIT_TOKEN_ENV, GitCredential, GitCredentialOperation, GitCredentialProvider,
    GitCredentialRequest, GitPolicy, GitShellCommandRequest, GitShellCredential, GitTool,
    GitToolKind, GitWorkspaceConfig, LocalCommandBackend, LocalExecutionBackend,
    LocalWorkspaceFileBackend, LspLanguageTool, LspQueryTool, MAX_MODEL_TOOL_OUTPUT_BYTES,
    MAX_TOOL_UI_PREVIEW_BYTES, MIN_MODEL_TOOL_OUTPUT_BATCH_TOKENS, NoContainerBackend,
    NoGitCredentialProvider, OutputTruncation, PlanExitTool, RegisteredTool,
    RegisteredToolSchemaError, SECRET_REDACTION_REPLACEMENT, SecretRedaction, SessionNoteTool,
    SessionNoteToolKind, ShellCommandTimeout, SubagentContext, TOOL_APPLY_PATCH,
    TOOL_APPLY_SESSION_NOTE_PATCH, TOOL_EXEC, TOOL_GIT_BRANCH, TOOL_GIT_COMMIT, TOOL_GIT_DIFF,
    TOOL_GIT_FETCH, TOOL_GIT_PUSH, TOOL_GIT_STATUS, TOOL_GIT_SYNC_DEFAULT_BRANCH,
    TOOL_GIT_WORKSPACE_INFO, TOOL_LIST_FILES, TOOL_READ_FILE, TOOL_READ_SESSION_NOTE,
    TOOL_SEARCH_FILES, TOOL_SEARCH_SESSION_NOTE, TOOL_UPDATE_TODO_LIST, TOOL_WRITE_SESSION_NOTE,
    TOOL_WRITE_STDIN, TodoListTool, Tool, ToolCachePolicy, ToolContext, ToolExecutionResult,
    ToolHistoryProjection, ToolInput, ToolInputSchemaField, ToolLifecyclePhase,
    ToolLifecycleProjection, ToolOutput, ToolOutputArtifactDescriptor,
    ToolOutputArtifactPathRequest, ToolOutputCapture, ToolOutputCaptureRequest,
    ToolOutputModelOutputRequest, ToolOutputStream, ToolOutputStreamCapture, ToolOutputStreamSizes,
    ToolRegistry, ToolRuntimeEvent, ToolRuntimeLockPolicy, TruncatedOutput, TruncationStrategy,
    TurnToolCacheHandle, WorkspaceAccess, WorkspaceBoundary, WorkspaceFileBackend,
    WorkspaceFileListEntry, WorkspaceFileListRequest, WorkspaceFileListResult,
    WorkspaceFileReadRequest, WorkspaceFileRemoveRequest, WorkspaceFileSearchMatch,
    WorkspaceFileSearchRequest, WorkspaceFileSearchResult, WorkspaceFileStat,
    WorkspaceFileStatRequest, WorkspaceFileTool, WorkspaceFileToolExecution, WorkspaceFileToolKind,
    WorkspaceFileWriteRequest, WorkspaceMutability, WriteStdinTool, command_output_model_path,
    enforce_model_output_limit, enforce_model_output_limit_with_cap, execute_workspace_file_tool,
    function_tool_schema, git_askpass_script, git_shell_command, git_shell_credential_prelude,
    git_shell_retry_function, lsp_tool_for_language, model_tool_output_batch_token_budget,
    model_visible_tool_output, model_visible_tool_output_batch_with_tokens,
    model_visible_tool_output_with_budget, model_visible_tool_output_with_bytes,
    model_visible_tool_output_with_tokens, redacted_trace_preview_value,
    run_tool_backend_with_cancellation, shell_command_with_timeout, shell_quote_word,
    strict_tool_input_schema, tool_history_projection, tool_lifecycle_projection,
    tool_lifecycle_projections, tool_output_artifact_file_path, trace_preview_output,
    trace_preview_value,
};
pub use trace::TraceRecorder;
pub use turn::{
    AGENT_MAX_COUNT, AGENT_MAX_DEPTH, DEFAULT_WALL_CLOCK_MS, InteractionCallback,
    InteractionFuture, PermissionMode, ToolApprovalDecision, ToolApprovalRequest, ToolEffect,
    ToolExecutionMode, TurnAbortReason, TurnBudget, TurnOptions, TurnRequest, TurnResult,
    TurnResultStatus, UserInputMode,
};
pub use web_search::{
    ToolVisibilityConstraint, WebSearchAvailability, WebSearchBackend, WebSearchPath,
    WebSearchPlan, WebSearchResolution, plan_web_search,
};
pub use working_set::{
    CONVERSATION_RECOVERY_SECTION_ID, CURRENT_TODO_SECTION_ID, EVIDENCE_LEDGER_SECTION_ID,
    MAX_PINNED_CONTEXT_BYTES, MAX_PINNED_SECTION_BYTES, MAX_SESSION_NOTE_BYTES,
    REVIEW_CHECKPOINT_SECTION_ID, REVIEW_MANIFEST_SECTION_ID, TurnWorkingSetChange,
    TurnWorkingSetHandle, canonical_content_hash, canonical_json_hash, context_section,
};
pub use workspace::{load_workspace_instructions, resolve_workspace_root};
