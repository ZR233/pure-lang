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

export type ModelRecord = {
  slug: string;
  displayName: string;
  description?: string | null;
  contextWindow?: number | null;
  maxContextWindow?: number | null;
  autoCompactTokenLimit?: number | null;
  defaultTemperature?: number | null;
  maxOutputTokens?: number | null;
  reasoningEfforts: string[];
  capabilities?: string[];
  inputModalities?: string[];
  truncationMode?: string;
  truncationLimit?: number;
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
  templates: ProviderTemplateRecord[];
  configExists: boolean;
};

export type ProviderSettingsInput = {
  defaultProviderId?: string | null;
  providers: ProviderInput[];
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

export type BootstrapPayload = {
  projects: ProjectRecord[];
  selectedProjectId?: string | null;
  sessions: SessionRecord[];
  selectedSessionId?: string | null;
  messages: ChatMessage[];
  config: ConfigPayload;
};

export type ProjectSelectionPayload = {
  projectId: string;
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  selectedSessionId?: string | null;
  messages: ChatMessage[];
};

export type SessionSelectionPayload = {
  sessionId: string;
  sessions: SessionRecord[];
  messages: ChatMessage[];
};

export type RunPromptResponse = {
  sessionId: string;
  sessions: SessionRecord[];
  messages: ChatMessage[];
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
