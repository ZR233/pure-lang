export type ProjectRecord = {
  id: string;
  name: string;
  path: string;
  updatedAt: number;
};

export type SessionRecord = {
  id: string;
  projectId: string;
  title: string;
  mode: string;
  updatedAt: number;
};

export type ChatMessage = {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  reasoningContent?: string | null;
  metadata?: Record<string, string> | null;
};

export type ToolCallStatus =
  | "streaming"
  | "completed"
  | "pending_approval"
  | "approved"
  | "denied"
  | "result_ready";

export type TrackedToolCall = {
  id: string;
  name: string;
  arguments: string;
  status: ToolCallStatus;
  workingDirectory?: string | null;
  result?: string | null;
  startedAt: number;
};

export type ChatItem =
  | { kind: "message"; message: ChatMessage; key: string }
  | { kind: "tool_call"; toolCall: TrackedToolCall; key: string };

export type SubagentStatus =
  | "queued"
  | "awaitingApproval"
  | "running"
  | "awaitingToolApproval"
  | "succeeded"
  | "failed"
  | "denied";

export type SubagentActivity = {
  eventId: string;
  id: string;
  parentId?: string | null;
  role: string;
  task: string;
  status: SubagentStatus;
  summary?: string | null;
  depth: number;
  error?: string | null;
  updatedAt: number;
};

export type SubagentEventPayload = Omit<SubagentActivity, "eventId"> & {
  eventId?: string;
};

export type ProviderRecord = {
  id: string;
  templateKind: ProviderKind;
  name: string;
  subtitle: string;
  status: string;
  baseUrl: string;
  bearerToken: string;
  defaultModel: string;
  modelCount: string;
  updatedAt: string;
  wireApi: string;
  models: ModelRecord[];
  defaultModels: ModelRecord[];
  customModels: ModelRecord[];
};

export type ProviderKind = "deepseek" | "openai";

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

export type SessionRuntime = {
  sessionId: string;
  model: string;
  contextWindow?: number | null;
  latestContextTokens: number;
  promptTokens: number;
  completionTokens: number;
  cachedPromptTokens: number;
  totalTokens: number;
  cacheHitRate?: number | null;
  currency?: string | null;
  inputPricePerMTok?: number | null;
  outputPricePerMTok?: number | null;
  cacheReadPricePerMTok?: number | null;
  estimatedCost?: number | null;
  activeSkills: string[];
  activeMcpServers: string[];
  updatedAt: number;
};

export type ProviderTemplateRecord = {
  id: ProviderKind;
  name: string;
  baseUrl: string;
  defaultModel: string;
  wireApi: string;
  defaultModels: ModelRecord[];
};

export type ConfigPayload = {
  toml: string;
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

export type ProviderInput = {
  id: string;
  templateKind: ProviderKind;
  name: string;
  baseUrl: string;
  bearerToken: string;
  defaultModel: string;
  wireApi: string;
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
  messages: ChatMessage[];
  subagentEvents: SubagentActivity[];
  sessionRuntime?: SessionRuntime | null;
  config: ConfigPayload;
};

export type ProjectSelectionPayload = {
  projectId: string;
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  selectedSessionId?: string | null;
  messages: ChatMessage[];
  subagentEvents: SubagentActivity[];
  sessionRuntime?: SessionRuntime | null;
};

export type SessionSelectionPayload = {
  sessionId: string;
  sessions: SessionRecord[];
  messages: ChatMessage[];
  subagentEvents: SubagentActivity[];
  sessionRuntime?: SessionRuntime | null;
};

export type RunPromptResponse = {
  sessionId: string;
  sessions: SessionRecord[];
  messages: ChatMessage[];
  subagentEvents: SubagentActivity[];
  sessionRuntime: SessionRuntime;
};

export type AgentEvent =
  | { textDelta: { content: string } }
  | { thinkingDelta: { content: string } }
  | { toolCallDelta: { id: string; name: string; argumentsDelta: string } }
  | { toolCallComplete: { id: string; name: string; arguments: string } }
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
  | { subagentStateChanged: SubagentEventPayload }
  | "turnStarted"
  | "done"
  | { error: { message: string; severity: string } };

export type AgentEventPayload = {
  sessionId: string;
  event: AgentEvent;
};

export type ToolApprovalRequest = {
  approvalId: string;
  sessionId: string;
  name: string;
  arguments: unknown;
  workingDirectory?: string | null;
  parentSubagentId?: string | null;
};

export type ToolApprovalResolved = {
  approvalId: string;
  decision: "approved" | "denied";
  reason?: string | null;
};

export type PromptFailed = {
  sessionId?: string | null;
  message: string;
};
