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
  name: string;
  subtitle: string;
  status: string;
  baseUrl: string;
  modelCount: string;
  updatedAt: string;
  wireApi: string;
};

export type ConfigPayload = {
  toml: string;
  providers: ProviderRecord[];
  configExists: boolean;
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
