import type {
  AgentEvent,
  AgentActivity,
  AgentActivityPayload,
  BootstrapPayload,
  ChatMessage,
  ConfigPayload,
  ProjectRecord,
  ProjectSelectionPayload,
  ProviderRecord,
  ProviderTemplateRecord,
  RoleRecord,
  RunPromptResponse,
  SessionRecord,
  SessionRuntime,
  SessionSelectionPayload,
  TimelineItem,
  TurnPhase,
  TurnStatus,
  ToolApprovalRequest,
  ToolApprovalResolved,
  TrackedToolCall,
} from "../types";

export type SettingsTab = "providers" | "models" | "roles" | "security" | "general";

export type StudioState = {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  messages: ChatMessage[];
  providers: ProviderRecord[];
  roles: RoleRecord[];
  providerTemplates: ProviderTemplateRecord[];
  selectedProjectId: string | null;
  selectedSessionId: string | null;
  manualPath: string;
  prompt: string;
  status: string;
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
  isBusy: boolean;
  streamingText: string;
  thinkingText: string;
  toolCalls: Map<string, TrackedToolCall>;
  agentActivities: AgentActivity[];
  timelineItems: TimelineItem[];
  sessionRuntime: SessionRuntime | null;
  approvals: ToolApprovalRequest[];
  settingsOpen: boolean;
  activeSettingsTab: SettingsTab;
  providerSearch: string;
  selectedProviderId: string | null;
  configToml: string;
  configExists: boolean;
};

export type StudioAction =
  | { type: "bootstrapLoaded"; payload: BootstrapPayload; status: string }
  | { type: "bootstrapFailed"; status: string }
  | { type: "timelineLoaded"; items: TimelineItem[] }
  | { type: "timelineLoadFailed"; status: string }
  | { type: "projectSelectionLoaded"; payload: ProjectSelectionPayload; status: string }
  | { type: "sessionSelectionLoaded"; payload: SessionSelectionPayload; status: string }
  | { type: "runPromptLoaded"; payload: RunPromptResponse; status: string }
  | { type: "runPromptFailed"; status: string }
  | { type: "setBusy"; value: boolean }
  | { type: "setPrompt"; prompt: string }
  | { type: "setManualPath"; path: string }
  | { type: "setProviderSearch"; search: string }
  | { type: "setSelectedProviderId"; providerId: string | null }
  | { type: "setRoles"; roles: RoleRecord[] }
  | { type: "setProviders"; providers: ProviderRecord[] }
  | { type: "setConfigToml"; toml: string }
  | { type: "setSettingsOpen"; value: boolean; tab?: SettingsTab }
  | { type: "configLoaded"; payload: ConfigPayload; status?: string }
  | { type: "enqueueApproval"; payload: ToolApprovalRequest; status: string }
  | { type: "resolveApproval"; payload: ToolApprovalResolved; status: string }
  | { type: "stopRequested"; status: string }
  | { type: "stopFallback"; status: string }
  | { type: "agentEvent"; event: AgentEvent; statusText: string }
  | { type: "appendUserPrompt"; content: string; status: string; startedAt: number };

export const initialStudioState = (startingStatus: string): StudioState => ({
  projects: [],
  sessions: [],
  messages: [],
  providers: [],
  roles: [],
  providerTemplates: [],
  selectedProjectId: null,
  selectedSessionId: null,
  manualPath: "",
  prompt: "",
  status: startingStatus,
  turnPhase: "idle",
  turnStartedAt: null,
  isBusy: false,
  streamingText: "",
  thinkingText: "",
  toolCalls: new Map(),
  agentActivities: [],
  timelineItems: [],
  sessionRuntime: null,
  approvals: [],
  settingsOpen: false,
  activeSettingsTab: "providers",
  providerSearch: "",
  selectedProviderId: null,
  configToml: "",
  configExists: false,
});

export function studioReducer(state: StudioState, action: StudioAction): StudioState {
  switch (action.type) {
    case "bootstrapLoaded":
      return {
        ...state,
        projects: action.payload.projects,
        selectedProjectId: action.payload.selectedProjectId ?? null,
        sessions: action.payload.sessions,
        selectedSessionId: action.payload.selectedSessionId ?? null,
        messages: action.payload.messages,
        agentActivities: mergeAgentActivities([], action.payload.agentEvents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        ...configFields(state.selectedProviderId, action.payload.config),
        status: action.status,
        turnPhase: "idle",
      };
    case "bootstrapFailed":
      return { ...state, status: action.status };
    case "timelineLoaded":
      return { ...state, timelineItems: action.items };
    case "timelineLoadFailed":
      return { ...state, status: action.status };
    case "projectSelectionLoaded":
      return {
        ...state,
        projects: action.payload.projects,
        selectedProjectId: action.payload.projectId,
        sessions: action.payload.sessions,
        selectedSessionId: action.payload.selectedSessionId ?? null,
        messages: action.payload.messages,
        agentActivities: mergeAgentActivities([], action.payload.agentEvents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        timelineItems: [],
        streamingText: "",
        thinkingText: "",
        toolCalls: new Map(),
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
      };
    case "sessionSelectionLoaded":
      return {
        ...state,
        sessions: action.payload.sessions.length > 0 ? action.payload.sessions : state.sessions,
        selectedSessionId: action.payload.sessionId,
        messages: action.payload.messages,
        agentActivities: mergeAgentActivities([], action.payload.agentEvents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        timelineItems: [],
        streamingText: "",
        thinkingText: "",
        toolCalls: new Map(),
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
      };
    case "runPromptLoaded":
      return {
        ...state,
        selectedSessionId: action.payload.sessionId,
        sessions: action.payload.sessions,
        messages: action.payload.messages,
        agentActivities: mergeAgentActivities([], action.payload.agentEvents ?? []),
        sessionRuntime: action.payload.sessionRuntime,
        timelineItems: mergeTimelineItems(state.timelineItems, action.payload.timelineItems),
        streamingText: "",
        thinkingText: "",
        toolCalls: new Map(),
        turnPhase: phaseForTurnStatus(action.payload.turnStatus),
        turnStartedAt: null,
        status: action.status,
        isBusy: false,
      };
    case "runPromptFailed":
      return {
        ...state,
        status: action.status,
        turnPhase: "failed",
        turnStartedAt: null,
        isBusy: false,
      };
    case "setBusy":
      return { ...state, isBusy: action.value };
    case "setPrompt":
      return { ...state, prompt: action.prompt };
    case "setManualPath":
      return { ...state, manualPath: action.path };
    case "setProviderSearch":
      return { ...state, providerSearch: action.search };
    case "setSelectedProviderId":
      return { ...state, selectedProviderId: action.providerId };
    case "setRoles":
      return { ...state, roles: action.roles };
    case "setProviders":
      return { ...state, providers: action.providers };
    case "setConfigToml":
      return { ...state, configToml: action.toml };
    case "setSettingsOpen":
      return {
        ...state,
        settingsOpen: action.value,
        activeSettingsTab: action.tab ?? state.activeSettingsTab,
      };
    case "configLoaded":
      return {
        ...state,
        ...configFields(state.selectedProviderId, action.payload),
        status: action.status ?? state.status,
      };
    case "enqueueApproval":
      return {
        ...state,
        approvals: [action.payload, ...state.approvals],
        status: action.status,
        turnPhase: "approval",
      };
    case "resolveApproval":
      return {
        ...state,
        approvals: state.approvals.filter((approval) => approval.approvalId !== action.payload.approvalId),
        status: action.status,
        turnPhase: state.turnPhase === "stopping" ? "stopping" : "tool",
      };
    case "stopRequested":
      return { ...state, turnPhase: "stopping", status: action.status };
    case "stopFallback":
      return {
        ...state,
        status: action.status,
        turnPhase: "interrupted",
        turnStartedAt: null,
        isBusy: false,
      };
    case "appendUserPrompt":
      return {
        ...state,
        prompt: "",
        isBusy: true,
        streamingText: "",
        thinkingText: "",
        messages: [...state.messages, { role: "user", content: action.content }],
        status: action.status,
        turnPhase: "running",
        turnStartedAt: action.startedAt,
      };
    case "agentEvent":
      return reduceAgentEvent(state, action.event, action.statusText);
    default:
      return state;
  }
}

function reduceAgentEvent(state: StudioState, event: AgentEvent, statusText: string): StudioState {
  if (event === "turnStarted") {
    return {
      ...state,
      status: statusText,
      turnPhase: "running",
      turnStartedAt: Date.now(),
    };
  }
  if (event === "done") {
    return {
      ...state,
      status: statusText,
      turnPhase:
        state.turnPhase === "interrupted" || state.turnPhase === "failed"
          ? state.turnPhase
          : "completed",
      turnStartedAt: null,
    };
  }
  if ("turnInterrupted" in event) {
    return {
      ...state,
      status: statusText,
      turnPhase: "interrupted",
    };
  }
  if ("textDelta" in event) {
    return {
      ...state,
      streamingText: state.streamingText + event.textDelta.content,
      status: statusText,
      turnPhase: "running",
    };
  }
  if ("thinkingDelta" in event) {
    return {
      ...state,
      thinkingText: state.thinkingText + event.thinkingDelta.content,
      status: statusText,
      turnPhase: "thinking",
    };
  }
  if ("toolCallDelta" in event) {
    const next = new Map(state.toolCalls);
    const existing = next.get(event.toolCallDelta.id);
    if (existing) {
      next.set(event.toolCallDelta.id, {
        ...existing,
        arguments: existing.arguments + event.toolCallDelta.argumentsDelta,
      });
    } else {
      next.set(event.toolCallDelta.id, {
        id: event.toolCallDelta.id,
        name: event.toolCallDelta.name,
        arguments: event.toolCallDelta.argumentsDelta,
        status: "streaming",
        startedAt: Date.now(),
      });
    }
    return {
      ...state,
      status: statusText,
      turnPhase: "tool",
      toolCalls: next,
    };
  }
  if ("toolCallComplete" in event) {
    const next = new Map(state.toolCalls);
    const existing = next.get(event.toolCallComplete.id);
    if (existing) {
      next.set(event.toolCallComplete.id, {
        ...existing,
        name: event.toolCallComplete.name || existing.name,
        status: "completed",
        arguments: event.toolCallComplete.arguments,
      });
    } else {
      next.set(event.toolCallComplete.id, {
        id: event.toolCallComplete.id,
        name: event.toolCallComplete.name,
        arguments: event.toolCallComplete.arguments,
        status: "completed",
        startedAt: Date.now(),
      });
    }
    return {
      ...state,
      turnPhase: "tool",
      toolCalls: next,
    };
  }
  if ("toolApprovalGranted" in event) {
    const next = new Map(state.toolCalls);
    const existing = next.get(event.toolApprovalGranted.id);
    if (existing) {
      next.set(event.toolApprovalGranted.id, { ...existing, status: "approved" });
    }
    return {
      ...state,
      status: statusText,
      turnPhase: "tool",
      toolCalls: next,
    };
  }
  if ("toolApprovalDenied" in event) {
    const next = new Map(state.toolCalls);
    const existing = next.get(event.toolApprovalDenied.id);
    if (existing) {
      next.set(event.toolApprovalDenied.id, { ...existing, status: "denied" });
    }
    return {
      ...state,
      status: statusText,
      turnPhase: state.turnPhase === "stopping" ? "stopping" : "tool",
      toolCalls: next,
    };
  }
  if ("agentStateChanged" in event) {
    return {
      ...state,
      agentActivities: mergeAgentActivities(state.agentActivities, [event.agentStateChanged]),
      turnPhase: "subagent",
      status: statusText,
    };
  }
  if ("error" in event) {
    return {
      ...state,
      status: statusText,
      turnPhase: "failed",
    };
  }
  return state;
}

export function mergeTimelineItems(current: TimelineItem[], incoming: TimelineItem[]): TimelineItem[] {
  if (incoming.length === 0) return current;
  const bySeq = new Map(current.map((item) => [item.sequence, item]));
  for (const item of incoming) {
    bySeq.set(item.sequence, item);
  }
  return [...bySeq.values()].sort((a, b) => a.sequence - b.sequence);
}

export function mergeAgentActivities(
  current: AgentActivity[],
  events: AgentActivityPayload[],
): AgentActivity[] {
  const byId = new Map(current.map((activity) => [activity.id, activity]));
  for (const event of events) {
    byId.set(event.id, normalizeAgentActivity(event));
  }
  return [...byId.values()].sort((left, right) => {
    if (left.path !== right.path) {
      return left.path.localeCompare(right.path);
    }
    return right.updatedAt - left.updatedAt;
  });
}

export function normalizeAgentActivity(event: AgentActivityPayload): AgentActivity {
  return {
    eventId:
      event.eventId ??
      `${event.id}-${event.updatedAt}-${event.status}-${Math.random().toString(16).slice(2)}`,
    id: event.id,
    path: event.path,
    parentPath: event.parentPath ?? null,
    role: event.role,
    task: event.task,
    status: event.status,
    summary: event.summary ?? null,
    depth: event.depth,
    error: event.error ?? null,
    updatedAt: event.updatedAt,
  };
}

function configFields(selectedProviderId: string | null, payload: ConfigPayload) {
  const nextProviderId =
    selectedProviderId && payload.providers.some((provider) => provider.id === selectedProviderId)
      ? selectedProviderId
      : payload.providers[0]?.id ?? null;
  return {
    providers: payload.providers,
    roles: payload.roles,
    providerTemplates: payload.templates,
    configToml: payload.toml,
    configExists: payload.configExists,
    selectedProviderId: nextProviderId,
  };
}

export function phaseForTurnStatus(status: TurnStatus): TurnPhase {
  switch (status) {
    case "started":
      return "running";
    case "completed":
      return "completed";
    case "failed":
      return "failed";
    case "interrupted":
      return "interrupted";
  }
}
