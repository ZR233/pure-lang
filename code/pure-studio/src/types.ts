export type ProjectRecord = {
  id: string;
  name: string;
  path: string;
  updatedAt: number;
};

export type CompileMode = "auto" | "plan";

export type PermissionMode =
  | "request-approval"
  | "auto-review"
  | "full-access";

export type SessionRecord = {
  id: string;
  projectId: string;
  title: string;
  mode: string;
  updatedAt: number;
  visibility: "active" | "handoffOrigin" | "archived";
};

export type AgentStatus =
  | "queued"
  | "running"
  | "waiting"
  | "completed"
  | "errored"
  | "interrupted"
  | "shutdown"
  | "notFound";

export type AgentDto = {
  id: string;
  sessionId: string;
  path: string;
  parentPath?: string | null;
  role: string;
  task: string;
  status: AgentStatus;
  summary?: string | null;
  depth: number;
  error?: string | null;
  reason?: string | null;
  budgetLimitKind?: string | null;
  budgetUsage?: {
    modelSteps: number;
    toolCalls: number;
    waitCalls: number;
    elapsedMs: number;
  } | null;
  runtimeUsage?: RuntimeUsage | null;
  updatedAt: number;
};

export type AgentStateChangedEvent = {
  id: string;
  path: string;
  parentPath?: string | null;
  role: string;
  task: string;
  status: AgentStatus;
  summary?: string | null;
  depth: number;
  error?: string | null;
  reason?: string | null;
  budgetLimitKind?: string | null;
  budgetUsage?: {
    modelSteps: number;
    toolCalls: number;
    waitCalls: number;
    elapsedMs: number;
  } | null;
  updatedAt: number;
};

export type AgentTimelineEvent = {
  eventId: string;
  sessionId: string;
  sequence: number;
  kind: string;
  agentId?: string | null;
  path?: string | null;
  parentPath?: string | null;
  payload: Record<string, unknown> | null;
  createdAt: number;
};

export type ProviderRecord = {
  id: string;
  templateKind: ProviderKind;
  name: string;
  subtitle: string;
  status: string;
  baseUrl: string;
  bearerToken: string;
  hasBearerToken?: boolean;
  defaultModel: string;
  modelCount: string;
  updatedAt: string;
  providerKind: string;
  models: ModelRecord[];
  defaultModels: ModelRecord[];
  customModels: ModelRecord[];
};

export type ProviderUsageStatus = "ready" | "unsupported" | "missingCredential" | "failed";

export type ProviderUsageRecord = {
  providerId: string;
  updatedAt: number;
  status: ProviderUsageStatus;
  usageKind: "deepseekBalance" | "zhipuCodingPlan" | "unsupported" | "unknown";
  message?: string | null;
  balance?: DeepSeekBalanceUsage | null;
  codingPlan?: ZhipuCodingPlanUsage | null;
};

export type DeepSeekBalanceUsage = {
  isAvailable: boolean;
  balances: DeepSeekBalanceInfo[];
};

export type DeepSeekBalanceInfo = {
  currency: string;
  totalBalance: string;
  grantedBalance: string;
  toppedUpBalance: string;
};

export type ZhipuCodingPlanUsage = {
  level?: string | null;
  limits: ZhipuQuotaLimit[];
};

export type ZhipuQuotaLimit = {
  window: "fiveHour" | "weekly" | "mcpMonthly" | "other";
  label: string;
  percentage: number;
  currentValue?: number | null;
  total?: number | null;
  remaining?: number | null;
  nextResetAt?: number | null;
  usageDetails: ZhipuToolUsageDetail[];
};

export type ZhipuToolUsageDetail = {
  name: string;
  currentValue?: number | null;
  total?: number | null;
  percentage?: number | null;
};

export type ProviderKind = "deepseek" | "openai" | "zhipu" | "zhipu-coding-plan";

export type RoleKey = "explorer" | "planner" | "executor" | "reviewer";

export type RoleRecord = {
  key: RoleKey;
  displayName: string;
  provider: string;
  model: string;
  effort: string;
};

export type ModelRecord = {
  slug: string;
  displayName: string;
  description?: string | null;
  contextWindow?: number | null;
  maxContextWindow?: number | null;
  autoCompactTokenLimit?: number | null;
  defaultTemperature?: number | null;
  maxOutputTokens?: number | null;
  currency?: string | null;
  inputPricePerMTok?: number | null;
  outputPricePerMTok?: number | null;
  cacheReadPricePerMTok?: number | null;
  reasoningEfforts: string[];
  capabilities: ModelCapabilities;
  truncationMode?: string;
  truncationLimit?: number;
  baseInstructions?: string;
};

export type ModelModality = "text" | "image" | "audio" | "video" | "pdf";

export type ModelCapabilities = {
  streaming: boolean;
  temperature: boolean;
  reasoning: boolean;
  webSearch: boolean;
  input: ModelModality[];
  output: ModelModality[];
  tools: {
    functionCalling: boolean;
    parallelToolCalls: boolean;
    customTools: boolean;
    freeformTools: boolean;
  };
  interleaved?: { field: "reasoning" | "reasoning_content" | "reasoning_details" } | null;
};

export type AttachmentRecord = {
  id: string;
  sessionId: string;
  mediaType: string;
  filename?: string | null;
  byteSize: number;
  width?: number | null;
  height?: number | null;
  createdAt: number;
  dataUrl: string;
};

export type SkillScope = "project" | "user" | "system" | "external";

export type SkillRecord = {
  name: string;
  description: string;
  category?: string | null;
  platforms: string[];
  scope: SkillScope;
  path: string;
};

export type DiscoveredSkillsPayload = {
  projectDir: string;
  skills: SkillRecord[];
  warnings: string[];
};

export type McpTransport = "stdio" | "streamableHttp";
export type McpServerSourceKind = "user" | "builtIn";
export type McpServerStatusKind = "enabled" | "disabled" | "missingCredential";
export type McpServerMutationPolicy = "userEditable" | "lockedIdentity";
export type McpAvailabilityKind =
  | "checking"
  | "available"
  | "unavailable"
  | "disabled"
  | "missingCredential";
export type LspAvailabilityKind =
  | "checking"
  | "available"
  | "unavailable"
  | "missingCommand"
  | "disabled";

export type LspActivityKind = "idle" | "busy" | "indexing";

export type KeyValuePair = {
  key: string;
  value: string;
};

export type McpServerRecord = {
  id: string;
  enabled: boolean;
  transport: McpTransport;
  command?: string | null;
  args: string[];
  env: KeyValuePair[];
  cwd?: string | null;
  url?: string | null;
  bearerTokenEnvVar?: string | null;
  headers: KeyValuePair[];
  endpoint: string;
  sourceKind: McpServerSourceKind;
  sourceLabel: string;
  sourceDetail?: string | null;
  statusKind: McpServerStatusKind;
  statusMessage?: string | null;
  mutationPolicy: McpServerMutationPolicy;
  availabilityKind: McpAvailabilityKind;
  availabilityMessage?: string | null;
  lastCheckedAt?: number | null;
  toolCount?: number | null;
};

export type LspServerRecord = {
  id: string;
  displayName: string;
  extensions: string[];
  languageIds: string[];
  availabilityKind: LspAvailabilityKind;
  availabilityMessage?: string | null;
  lastCheckedAt?: number | null;
  diagnosticCount: number;
  activityKind: LspActivityKind;
  activityTitle?: string | null;
  activityMessage?: string | null;
  activityPercentage?: number | null;
  lastError?: string | null;
  lastErrorAt?: number | null;
};

export type RuntimeCostAmount = {
  currency: string;
  amount: number;
};

export type RuntimeUsage = {
  model: string;
  contextWindow?: number | null;
  latestContextTokens: number;
  promptTokens: number;
  completionTokens: number;
  cachedPromptTokens: number;
  totalTokens: number;
  cacheHitRate?: number | null;
  estimatedCosts: RuntimeCostAmount[];
  hasUnpricedUsage: boolean;
  updatedAt: number;
};

export type SessionRuntime = {
  sessionId: string;
  usage: RuntimeUsage;
  activeSkills: string[];
  activeMcpServers: string[];
  activeLspServers: string[];
  updatedAt: number;
};

export type ToolCallStatus2 =
  | "started"
  | "streaming"
  | "awaitingApproval"
  | "approved"
  | "denied"
  | "running"
  | "completed"
  | "failed"
  | "interrupted"
  | "budgetLimited";

export type TurnStatus = "started" | "completed" | "aborted" | "errored";

export type StudioTurnStatus =
  | "queued"
  | "contextLoading"
  | "waitingForModel"
  | "streaming"
  | "waitingForInteraction"
  | "runningTool"
  | "persisting"
  | "completed"
  | "failed"
  | "cancelled";

export type TurnPhase =
  | "idle"
  | "running"
  | "thinking"
  | "tool"
  | "subagent"
  | "approval"
  | "userInput"
  | "stopping"
  | "completed"
  | "aborted"
  | "interrupted"
  | "budgetLimited"
  | "errored"
  | "failed";

export type UsageSnapshot = {
  promptTokens: number;
  completionTokens: number;
  cachedPromptTokens: number;
  totalTokens: number;
};

export type TimelineTextChannel = "user" | "commentary" | "final";

export type TimelineAttachment = {
  id: string;
  mediaType: string;
  filename?: string | null;
  width?: number | null;
  height?: number | null;
  byteSize: number;
  dataUrl?: string | null;
};

export type TimelineItem = {
  turnId: string;
  itemId: string;
  startedSequence: number;
  /**
   * 该 item 最后一次收到的 delta 事件 sequence，用于丢弃乱序/重复 delta，
   * 防止 broadcast Lagged 后跨 turn 串台与乱序累积。
   */
  lastDeltaSequence?: number;
  kind: "text" | "thinking" | "tool" | "agent" | "turn" | "inference" | "plan";
  status: ToolCallStatus2;
  createdAt: number;
  updatedAt: number;
  textChannel?: TimelineTextChannel | null;
  content: string;
  attachments?: TimelineAttachment[];
  thinkingChunks: { chunkIndex: number; content: string }[];
  tool?: {
    toolCallId: string;
    callId?: string | null;
    providerItemId?: string | null;
    name: string;
    arguments: string;
    result?: string | null;
    exitCode?: number | null;
    timedOut: boolean;
    workingDirectory?: string | null;
    denialReason?: string | null;
  } | null;
  agent?: {
    id: string;
    path: string;
    parentPath?: string | null;
    role: string;
    task: string;
    status: AgentStatus;
    summary?: string | null;
    depth: number;
    error?: string | null;
    reason?: string | null;
  } | null;
  inference?: {
    inferenceId: string;
    model: string;
  } | null;
  usage?: UsageSnapshot | null;
};

export type SessionTimeline = {
  sessionId: string;
  events: TimelineEventRecord[];
  planStates?: PlanState[];
  interactions?: InteractionRequest[];
  nextSequence: number;
};

export type PlanLifecycleState =
  | "pendingConfirmation"
  | "accepted"
  | "implementing"
  | "implemented"
  | "implementationFailed"
  | "continuedPlanning"
  | "dismissed"
  | "cancelled";

export type PlanState = {
  planId: string;
  state: PlanLifecycleState;
  turnId?: string | null;
  reason?: string | null;
  updatedAt: number;
};

export type PlanLifecycleResponse = {
  sessionId: string;
  planStates: PlanState[];
  timelineNextSequence: number;
};

export type PlanLifecycleEvent = {
  planId: string;
  state: PlanLifecycleState;
  turnId?: string | null;
  reason?: string | null;
  updatedAt: number;
};

export type InteractionKind = "userInput" | "toolApproval" | "planConfirmation";
export type InteractionStatus = "pending" | "resolved" | "cancelled" | "expired";

export type InteractionScope = {
  sessionId: string;
  turnId: string;
  itemId?: string | null;
  toolId?: string | null;
  agentPath?: string | null;
};

export type InteractionPayload =
  | { type: "userInput"; questions: UserQuestion[] }
  | {
      type: "toolApproval";
      name: string;
      arguments: unknown;
      workingDirectory?: string | null;
      parentAgentId?: string | null;
    }
  | { type: "planConfirmation"; planId: string; content: string };

export type ToolApprovalResolution = "approved" | "denied";
export type PlanConfirmationResolution =
  | "implementFreshContext"
  | "continuePlanning"
  | "dismiss";

export type InteractionResolution =
  | { type: "userInput"; answers: Record<string, UserInputAnswer> }
  | {
      type: "toolApproval";
      decision: ToolApprovalResolution;
      reason?: string | null;
    }
  | {
      type: "planConfirmation";
      decision: PlanConfirmationResolution;
      content?: string | null;
      reason?: string | null;
    };

export type InteractionRequest = {
  interactionId: string;
  kind: InteractionKind;
  status: InteractionStatus;
  scope: InteractionScope;
  payload: InteractionPayload;
  createdAt: number;
  updatedAt: number;
  resolvedAt?: number | null;
  resolution?: InteractionResolution | null;
};

export type InteractionChangedEvent = {
  interaction: InteractionRequest;
};

export type InteractionChangedPayload = {
  sessionId: string;
  event: InteractionChangedEvent;
};

export type ResolveInteractionResponse = {
  sessionId: string;
  interaction: InteractionRequest;
  planLifecycle?: PlanLifecycleResponse | null;
};

export type ProviderTemplateRecord = {
  id: ProviderKind;
  name: string;
  baseUrl: string;
  defaultModel: string;
  providerKind: string;
  defaultModels: ModelRecord[];
};

export type ConfigPayload = {
  toml: string;
  permissionMode: PermissionMode;
  instructions: InstructionsRecord;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  templates: ProviderTemplateRecord[];
  mcpServers: McpServerRecord[];
  configExists: boolean;
};

export type InstructionsRecord = {
  baseOverride: string;
  developer: string;
  user: string;
  projectDocMaxBytes: number;
  projectDocFallbackFilenames: string[];
};

export type ProviderUsagesPayload = {
  usages: ProviderUsageRecord[];
};

export type ProviderSettingsInput = {
  defaultProviderId?: string | null;
  providers: ProviderInput[];
  roles: RoleInput[];
};

export type InstructionsInput = InstructionsRecord;

export type ProviderSettingsSaveSnapshot = {
  selectedProviderId?: string | null;
  providers?: ProviderRecord[];
  roles?: RoleRecord[];
};

export type McpSettingsInput = {
  servers: McpServerInput[];
};

export type McpHealthUpdatedPayload = {
  mcpServers: McpServerRecord[];
  activeMcpServers: string[];
};

export type LspHealthUpdatedPayload = {
  lspServers: LspServerRecord[];
  activeLspServers: string[];
};

export type McpServerInput = {
  id: string;
  enabled: boolean;
  transport: McpTransport;
  command?: string | null;
  args: string[];
  env: KeyValuePair[];
  cwd?: string | null;
  url?: string | null;
  bearerTokenEnvVar?: string | null;
  headers: KeyValuePair[];
  sourceKind?: McpServerSourceKind;
  sourceLabel?: string;
  sourceDetail?: string | null;
  statusKind?: McpServerStatusKind;
  statusMessage?: string | null;
  mutationPolicy?: McpServerMutationPolicy;
};

export type ProviderInput = {
  id: string;
  templateKind: ProviderKind;
  name: string;
  baseUrl: string;
  bearerToken: string;
  defaultModel: string;
  providerKind: string;
  customModels: ProviderModelInput[];
};

export type ProviderModelInput = {
  slug: string;
  displayName: string;
  reasoningEfforts: string[];
  baseInstructions?: string;
};

export type RoleInput = {
  key: RoleKey;
  provider: string;
  model: string;
  effort: string;
};

export type BootstrapPayload = {
  projects: ProjectRecord[];
  selectedProjectId?: string | null;
  sessions: SessionRecord[];
  selectedSessionId?: string | null;
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime?: SessionRuntime | null;
  interactions?: InteractionRequest[];
  lspHealth?: LspHealthUpdatedPayload | null;
  config: ConfigPayload;
};

export type ProjectSelectionPayload = {
  projectId?: string | null;
  selectedProjectId?: string | null;
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  selectedSessionId?: string | null;
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime?: SessionRuntime | null;
  interactions?: InteractionRequest[];
  lspHealth?: LspHealthUpdatedPayload | null;
};

export type SessionSelectionPayload = {
  sessionId: string;
  sessions: SessionRecord[];
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime?: SessionRuntime | null;
  interactions?: InteractionRequest[];
};

export type RunPromptResponse = {
  sessionId: string;
  sessions: SessionRecord[];
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime: SessionRuntime;
  timelineEvents: TimelineEventRecord[];
  planStates?: PlanState[];
  interactions?: InteractionRequest[];
  timelineNextSequence: number;
  turnStatus: TurnStatus;
  turnAbortReason?: string | null;
  turnError?: string | null;
};

export type StopPromptResponse = {
  sessionId: string;
  stopped: boolean;
};

export type AgentEvent =
  | { timelineItemStarted: { item: TimelineItem } }
  | { timelineItemDelta: { event: TimelineItemDeltaEvent } }
  | { timelineItemCompleted: { item: TimelineItem } }
  | { timelineItemFailed: { item: TimelineItem; error: string } }
  | { interactionChanged: { event: InteractionChangedEvent } }
  | { agentStateChanged: AgentStateChangedEvent }
  | { agentRuntimeUpdated: { delta: AgentRuntimeDelta } }
  | { skillActivated: { activation: SkillActivation } }
  | { turnInterrupted: { reason: string } }
  | {
      turnBudgetLimited: {
        reason: string;
        limitKind: string;
        usage: {
          modelSteps: number;
          toolCalls: number;
          waitCalls: number;
          elapsedMs: number;
        };
      };
    }
  | "done"
  | { error: { message: string; severity: string } };

export type TimelineItemDeltaEvent = {
  turnId: string;
  itemId: string;
  startedSequence: number;
  kind: TimelineItem["kind"];
  status: TimelineItem["status"];
  createdAt: number;
  updatedAt: number;
  delta:
    | { type: "text"; textChannel: TimelineTextChannel; delta: string }
    | { type: "thinking"; chunkIndex: number; delta: string }
    | { type: "toolArguments"; delta: string }
    | { type: "toolResult"; delta: string }
    | { type: "plan"; delta: string };
};

export type TimelineTracePayload =
  | { type: "timelineItemStarted"; item: TimelineItem }
  | { type: "timelineItemDelta"; event: TimelineItemDeltaEvent }
  | { type: "timelineItemCompleted"; item: TimelineItem }
  | { type: "timelineItemFailed"; item: TimelineItem; error: string }
  | { type: "planLifecycleChanged"; event: PlanLifecycleEvent }
  | { type: "interactionChanged"; event: InteractionChangedEvent }
  | { type: "skillActivated"; activation: SkillActivation }
  | {
      type: "enabledToolsRecorded";
      event: { turnId: string; mode: string; tools: string[] };
    };

export type TimelineEventRecord = {
  id: string;
  sessionId: string;
  sequence: number;
  createdAt: number;
  kind: string;
  payload: TimelineTracePayload;
};

export type AgentRuntimeDelta = {
  inferenceId: string;
  agentId: string;
  path: string;
  parentPath?: string | null;
  role: string;
  model: string;
  contextWindow?: number | null;
  usage: UsageSnapshot;
  estimatedCosts: RuntimeCostAmount[];
  hasUnpricedUsage: boolean;
  updatedAt: number;
};

export type StudioTurn = {
  turnId: string;
  sessionId: string;
  status: StudioTurnStatus;
  reason?: string | null;
  updatedAt: number;
};

export type StudioTimelineChange =
  | { type: "started"; item: TimelineItem }
  | { type: "delta"; event: TimelineItemDeltaEvent }
  | { type: "completed"; item: TimelineItem }
  | { type: "failed"; item: TimelineItem; error: string };

export type StudioEventKind =
  | { type: "turnChanged"; turn: StudioTurn }
  | { type: "timelineChanged"; change: StudioTimelineChange }
  | { type: "interactionChanged"; event: InteractionChangedEvent }
  | { type: "agentChanged"; agent: { payload: AgentDto | AgentEvent } }
  | { type: "agentTimelineChanged"; event: { payload: AgentTimelineEvent | AgentEvent } }
  | { type: "sessionRuntimeChanged"; runtime: { payload: SessionRuntime | AgentEvent } }
  | { type: "skillActivated"; activation: SkillActivation }
  | { type: "planLifecycleChanged"; event: PlanLifecycleEvent }
  | {
      type: "sessionHandoffChanged";
      handoff: {
        originSessionId: string;
        targetSessionId: string;
        kind: string;
        status: string;
        planId?: string | null;
        updatedAt: number;
      };
    }
  | { type: "sessionListChanged"; sessions: SessionRecord[] }
  | { type: "mcpHealthChanged"; health: { payload: McpHealthUpdatedPayload } }
  | { type: "lspHealthChanged"; health: { payload: LspHealthUpdatedPayload } }
  | { type: "stale"; laggedEvents: number };

export type StudioEventEnvelope = {
  eventId: string;
  projectId?: string | null;
  sessionId?: string | null;
  turnId?: string | null;
  sequence: number;
  createdAt: number;
  kind: StudioEventKind;
};

export type SubmitPromptResponse = {
  sessionId: string;
  turnId: string;
  cursor: number;
};

export type StudioEventsPayload = {
  sessionId: string;
  events: StudioEventEnvelope[];
  nextSequence: number;
};

export type SessionStatePayload = {
  sessionId: string;
  session: SessionRecord;
  sessions: SessionRecord[];
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime: SessionRuntime;
  interactions: InteractionRequest[];
  timeline: SessionTimeline;
  events: StudioEventEnvelope[];
  eventNextSequence: number;
};

export type SkillActivation = {
  name: string;
  source: string;
  path: string;
  turnId: string;
  toolCallId: string;
  activatedAt: number;
};

export type UserQuestionOption = {
  label: string;
  description: string;
};

export type UserQuestion = {
  id: string;
  header: string;
  question: string;
  isOther?: boolean;
  isSecret?: boolean;
  options?: UserQuestionOption[] | null;
};

export type UserInputAnswer = {
  answers: string[];
};

export type UserInputResponse = {
  answers: Record<string, UserInputAnswer>;
};

