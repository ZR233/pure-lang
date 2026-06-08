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

export type ProviderKind = "deepseek" | "openai" | "zhipu";

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
  capabilities?: string[];
  inputModalities?: string[];
  truncationMode?: string;
  truncationLimit?: number;
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

export type TimelineItem = {
  turnId: string;
  itemId: string;
  sequence: number;
  kind: "text" | "thinking" | "tool" | "agent" | "turn" | "inference" | "plan";
  status: ToolCallStatus2;
  createdAt: number;
  updatedAt: number;
  role?: "user" | "assistant" | null;
  content: string;
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
  items: TimelineItem[];
  nextSequence: number;
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
  providers: ProviderRecord[];
  roles: RoleRecord[];
  templates: ProviderTemplateRecord[];
  configExists: boolean;
};

export type ProviderSettingsInput = {
  defaultProviderId?: string | null;
  providers: ProviderInput[];
  roles: RoleInput[];
};

export type ProviderSettingsSaveSnapshot = {
  selectedProviderId?: string | null;
  providers?: ProviderRecord[];
  roles?: RoleRecord[];
};

export type ProviderInput = {
  id: string;
  templateKind: ProviderKind;
  name: string;
  baseUrl: string;
  bearerToken: string;
  defaultModel: string;
  providerKind: string;
  customModels: ModelRecord[];
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
  config: ConfigPayload;
};

export type ProjectSelectionPayload = {
  projectId: string;
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  selectedSessionId?: string | null;
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime?: SessionRuntime | null;
};

export type SessionSelectionPayload = {
  sessionId: string;
  sessions: SessionRecord[];
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime?: SessionRuntime | null;
};

export type RunPromptResponse = {
  sessionId: string;
  sessions: SessionRecord[];
  agentEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime: SessionRuntime;
  timelineItems: TimelineItem[];
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
  | { timelineItemCompleted: { sequence: number; item: TimelineItem } }
  | { timelineItemFailed: { sequence: number; item: TimelineItem; error: string } }
  | {
      toolApprovalRequested: {
        id: string;
        name: string;
        arguments: string;
        workingDirectory?: string | null;
      };
    }
  | { toolApprovalGranted: { id: string; name: string } }
  | { toolApprovalDenied: { id: string; name: string; reason: string } }
  | {
      userInputRequested: {
        requestId: string;
        toolId: string;
        questions: UserQuestion[];
      };
    }
  | { userInputAnswered: { requestId: string } }
  | { agentStateChanged: AgentDto }
  | { agentRuntimeUpdated: { delta: AgentRuntimeDelta } }
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
  sequence: number;
  kind: TimelineItem["kind"];
  status: TimelineItem["status"];
  createdAt: number;
  updatedAt: number;
  delta:
    | { type: "text"; delta: string }
    | { type: "thinking"; chunkIndex: number; delta: string }
    | { type: "toolArguments"; delta: string }
    | { type: "toolResult"; delta: string }
    | { type: "plan"; delta: string };
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

export type AgentEventPayload = {
  sessionId: string;
  timelineStale?: boolean | null;
  laggedEvents?: number | null;
  event?: AgentEvent | null;
  timelineEvent?: AgentTimelineEvent | null;
  agent?: AgentDto | null;
  sessionRuntime?: SessionRuntime | null;
};

export type ToolApprovalRequest = {
  approvalId: string;
  sessionId: string;
  name: string;
  arguments: unknown;
  workingDirectory?: string | null;
  parentAgentId?: string | null;
};

export type ToolApprovalResolved = {
  approvalId: string;
  decision: "approved" | "denied";
  reason?: string | null;
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

export type UserInputRequest = {
  requestId: string;
  sessionId: string;
  toolId: string;
  questions: UserQuestion[];
};

export type UserInputResolved = {
  requestId: string;
};

export type PromptFailed = {
  sessionId?: string | null;
  message: string;
};
