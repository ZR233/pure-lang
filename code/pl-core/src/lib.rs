mod agent;
mod attachment;
mod config;
mod config_editor;
mod context_compaction;
mod core;
mod first_run;
mod instruction;
#[cfg(feature = "studio")]
pub mod interfaces;
mod mcp;
mod permission;
mod process;
mod provider_error;
mod provider_usage;
mod runtime_usage;
mod session;
mod skill;
#[cfg(feature = "studio")]
mod studio;
mod tool;
mod trace;
mod turn;
mod workspace;

pub use agent::{
    AgentHandle, AgentInputTurnMode, AgentMessage, AgentMessageMode, AgentMessageRequest,
    AgentPath, AgentRecord, AgentRunSpec, AgentSpawnInput, AgentStatus, AgentStatusUpdate,
    AgentSupervisor, AgentToolRegistrar, AgentTurnPresence, AgentWaitCompletion, AgentWaitOutcome,
    AgentWaitSnapshot, AgentWaitStatusKind,
};
pub use attachment::MaterializedAttachment;
pub use config::{
    BuiltinMcpServerState, ConfigPaths, ConfigStore, DEFAULT_PROJECT_DOC_MAX_BYTES,
    EffectiveMcpServerConfig, InstructionsConfig, McpServerConfig, McpServerMutationPolicy,
    McpServerSourceKind, McpServerStatusKind, McpServerTransport, ModelRole, ProviderConfig,
    PureConfig, ReasoningEffort, ResolvedRoleConfig, RoleConfig, RoleConfigs, RuntimeConfig,
    SkillsConfig, SystemSkillsConfig, ToolCapabilityConfig, active_mcp_server_names,
    builtin_mcp_server_ids, effective_mcp_servers, is_builtin_mcp_server_id,
    normalize_builtin_mcp_server_states, zhipu_coding_plan_token,
};
pub use config_editor::{
    ProviderEdit, ProviderModelEdit, ProviderSettingsEdit, RoleEdit, infer_provider_template_kind,
};
pub use context_compaction::{
    ContextCompactionConfig, ContextCompactionReplacement, ContextCompactionSnapshot,
    ContextCompactionTrigger, RecentInteractionTailConfig,
};
pub use core::{
    AgentBackendProfile, AgentKernel, AgentKernelBuilder, AgentKernelToolRequest,
    AgentKernelToolSet, CoreAgentProfile, CoreModelTurnClient, CoreModelTurnOptions,
    CoreModelTurnRequest, CoreRuntimeOptions, CoreRuntimeProfile, HostedSharedToolVisibility,
    NoAgentKernelToolSet, PureCore, PureCoreBuilder, SharedToolSchemaOptions, ToolProfile,
    ToolSetBuilder, ToolVisibilitySet, WorkspaceProfile, hosted_container_shared_tool_names,
    shared_tool_names, shared_tool_schemas, stream_session_completion_response,
};
pub use first_run::{
    FirstRunConfigDraft, FirstRunModelDraft, FirstRunProviderDraft, ProviderTemplateKind,
};
pub use instruction::{
    InstructionAssembler, InstructionAssemblyRequest, InstructionBlock, InstructionBundle,
    InstructionProfile, InstructionSnapshot, InstructionSource, InstructionSourceKind,
};
#[cfg(feature = "studio")]
pub use interfaces::{
    ConfigRepository, EventSink, RuntimeEventEmitter, SessionRepository, TurnSnapshotRepository,
};
pub use mcp::{McpAvailabilityKind, McpAvailabilitySnapshot, McpRuntimeRegistry};
pub use pl_lsp::{
    LspActivityKind, LspAvailabilityKind, LspDiagnostic, LspPosition, LspQuery, LspQueryOperation,
    LspQueryResult, LspRange, LspRuntimeRegistry, LspServerSnapshot,
};
pub use pl_model::{
    DeepSeekBalanceInfo, DeepSeekBalanceUsage, ModelCapabilities, ModelContinuationState,
    ModelInfo, ModelModality, ModelParameter, ModelRequestProfile, ProviderKind,
    ReasoningInterleaved, ReasoningInterleavedField, ToolCapabilities, ToolWirePolicy,
    TruncationMode, ZhipuCodingPlanUsage, ZhipuQuotaLimit, ZhipuQuotaWindow, ZhipuToolUsageDetail,
};
pub use pl_protocol::{
    AgentRuntimeDelta, BudgetLimitKind, BudgetUsage, ContentPart, ErrorSeverity, ImageSource,
    InteractionChangedEvent, InteractionKind, InteractionPayload, InteractionRequest,
    InteractionResolution, InteractionScope, InteractionStatus, Message, MessageContent,
    MessageRole, OutputStream, PermissionLevel, PipelineStage, PlanConfirmationResolution,
    PureError, Result, RuntimeCostAmount, RuntimeUsageSnapshot, SkillActivation, StudioAgentPart,
    StudioAttachment, StudioEventEnvelope, StudioEventKind, StudioFilePart, StudioInferencePart,
    StudioMessage, StudioMessageRole, StudioMessageStatus, StudioPart, StudioPartDelta,
    StudioPartDeltaField, StudioPartStatus, StudioPartType, StudioPlanPart, StudioTextChannel,
    StudioToolPart, StudioTurn, StudioTurnStatus, TokenUsageSnapshot, ToolApprovalResolution,
    UserInputAnswer, UserInputRequest, UserInputResponse, UserQuestion, UserQuestionOption,
};
pub use provider_usage::{
    ProviderUsageData, ProviderUsageRecord, ProviderUsageState, provider_usage_records,
};
pub use session::{CoreSession, repair_incomplete_tool_history};
pub use skill::{SkillCatalog, SkillMetadata, SkillSourceKind};
#[cfg(feature = "studio")]
pub use studio::{
    AgentSnapshotRecord, AgentTimelineEventRecord, AttachmentRecord, InteractionEmitter,
    InteractionEmitterFuture, InteractionRuntime, ProjectRecord, RunPromptRequest, SessionRecord,
    SessionRuntimeRecord, SessionSkillRecord, SessionVisibility, StudioActiveTurn,
    StudioEventFilter, StudioEventRuntime, StudioEventScope, StudioFilteredEventReceiver,
    StudioPlanImplementationLifecycle, StudioPromptOutcome, StudioResolveInteractionResponse,
    StudioRuntime, StudioRuntimeSnapshot, StudioRuntimeState, StudioRuntimeStatus,
    StudioStopPromptResponse, StudioStore, StudioSubmitPromptOptions, StudioSubmitPromptRequest,
    StudioSubmitPromptResponse, StudioUserPromptPresentation, resolution_matches_kind,
    studio_attachment,
};
#[cfg(feature = "docker-tools")]
pub use tool::DockerCliContainerBackend;
pub use tool::{
    AgentControlAgentRecord, AgentControlBackend, AgentControlListOutput, AgentControlListRequest,
    AgentControlMessageOutput, AgentControlPolicy, AgentControlSendInputOutput,
    AgentControlSendInputRequest, AgentControlSpawnOutput, AgentControlSpawnRequest,
    AgentControlTargetRequest, AgentControlTool, AgentControlToolKind, AgentControlWaitOutput,
    AgentControlWaitRequest, AllowAllAgentControlPolicy, AskUserTool, BashInput, BashTool,
    ContainerBackend, ContainerCopyFromRequest, ContainerCopyToRequest, ContainerExecOutput,
    ContainerExecRequest, ContainerTool, ContainerToolExecution, ContainerToolKind,
    ContainerWorkspaceFileBackend, DEFAULT_MODEL_TOOL_OUTPUT_TOKENS, ExecutionBackend,
    ExecutionOutput, ExecutionRequest, GIT_TOKEN_ENV, GitCredential, GitCredentialOperation,
    GitCredentialProvider, GitCredentialRequest, GitPolicy, GitTool, GitToolKind,
    GitWorkspaceConfig, HostMcpToolSpec, LocalExecutionBackend, LocalWorkspaceFileBackend,
    LspLanguageTool, LspQueryTool, McpListResourceTemplatesRequest, McpListResourcesRequest,
    McpReadResourceRequest, McpResourceBackend, McpResourceTool, McpResourceToolKind, McpTool,
    McpToolBackend, McpToolRequest, NoContainerBackend, NoGitCredentialProvider, OutputTruncation,
    PlanExitTool, RegisteredTool, ResumeAgentTool, SubagentContext, TOOL_APPLY_PATCH,
    TOOL_CLOSE_AGENT, TOOL_CONTAINER_COPY, TOOL_CONTAINER_EXEC, TOOL_GIT_BRANCH, TOOL_GIT_COMMIT,
    TOOL_GIT_DIFF, TOOL_GIT_FETCH, TOOL_GIT_PUSH, TOOL_GIT_STATUS, TOOL_GIT_SYNC_DEFAULT_BRANCH,
    TOOL_GIT_WORKSPACE_INFO, TOOL_LIST_AGENTS, TOOL_LIST_FILES, TOOL_LIST_MCP_RESOURCE_TEMPLATES,
    TOOL_LIST_MCP_RESOURCES, TOOL_READ_FILE, TOOL_READ_MCP_RESOURCE, TOOL_RESUME_AGENT,
    TOOL_SEARCH_FILES, TOOL_SEND_INPUT, TOOL_SPAWN_AGENT, TOOL_UPDATE_TODO_LIST, TOOL_WAIT_AGENT,
    TodoListTool, Tool, ToolContext, ToolExecutionResult, ToolHistoryProjection, ToolInput,
    ToolLifecyclePhase, ToolLifecycleProjection, ToolOutput, ToolOutputArtifactDescriptor,
    ToolOutputArtifactPathRequest, ToolOutputCapture, ToolOutputCaptureRequest,
    ToolOutputModelOutputRequest, ToolOutputStream, ToolOutputStreamCapture, ToolOutputStreamSizes,
    ToolRegistry, ToolRuntimeEvent, ToolRuntimeLockPolicy, TruncatedOutput, TruncationStrategy,
    WorkspaceAccess, WorkspaceFileBackend, WorkspaceFileListEntry, WorkspaceFileListRequest,
    WorkspaceFileListResult, WorkspaceFileReadRequest, WorkspaceFileRemoveRequest,
    WorkspaceFileSearchMatch, WorkspaceFileSearchRequest, WorkspaceFileSearchResult,
    WorkspaceFileStat, WorkspaceFileStatRequest, WorkspaceFileTool, WorkspaceFileToolExecution,
    WorkspaceFileToolKind, WorkspaceFileWriteRequest, WriteStdinTool, execute_container_tool,
    execute_workspace_file_tool, host_mcp_tool_schema, host_mcp_tool_schemas,
    lsp_tool_for_language, model_visible_tool_output, model_visible_tool_output_with_tokens,
    redacted_trace_preview_value, tool_history_projection, tool_lifecycle_projection,
    tool_lifecycle_projections, tool_output_artifact_file_path, trace_preview_output,
    trace_preview_value,
};
pub use trace::TraceRecorder;
pub use turn::{
    AGENT_MAX_COUNT, AGENT_MAX_DEPTH, ActiveTurnControl, ActiveTurnSlot, AgentBudget,
    AgentTurnCancellationGuard, AgentTurnCancellationOutcome, AgentTurnCompletionMutation,
    AgentTurnCompletionOutcome, AgentTurnCompletionTransition, AgentTurnCurrentGuard,
    AgentTurnCurrentOutcome, AgentTurnStartMutation, AgentTurnStartOutcome,
    AgentTurnStartTransition, AgentTurnStatusGuard, AgentTurnStatusMutation,
    AgentTurnStatusOutcome, AgentTurnStatusTransition, BudgetPolicy, CompileMode,
    DEFAULT_WALL_CLOCK_MS, InteractionCallback, InteractionFuture, PermissionMode,
    ToolApprovalDecision, ToolApprovalPolicy, ToolApprovalRequest, ToolExecutionMode,
    TurnAbortReason, TurnBudget, TurnOptions, TurnOutcome, TurnOutcomeStatus, TurnRequest,
    TurnResult, TurnResultStatus, TurnReturnError, TurnTaskHandle, UserInputMode,
};
pub use workspace::{load_workspace_instructions, resolve_workspace_root};
