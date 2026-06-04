import type {
  AgentDto,
  AgentEvent,
  AgentTimelineEvent,
  BootstrapPayload,
  ConfigPayload,
  PermissionMode,
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
  TimelineItemDeltaEvent,
  TurnPhase,
  TurnStatus,
  ToolApprovalRequest,
  ToolApprovalResolved,
} from "../types";
import {
  applyLiveTimelineEvent,
  emptyTimelineState,
  mergeRunPromptTimeline,
  mergeTimelineSnapshot,
  removeOptimisticTimelineItems,
  type TimelineStateSlice,
} from "./timeline-state";

export type SettingsTab = "providers" | "skills" | "roles" | "security" | "general";

export type StudioState = TimelineStateSlice & {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
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
  agents: AgentDto[];
  agentTimelineEvents: AgentTimelineEvent[];
  sessionRuntime: SessionRuntime | null;
  approvals: ToolApprovalRequest[];
  settingsOpen: boolean;
  activeSettingsTab: SettingsTab;
  providerSearch: string;
  selectedProviderId: string | null;
  configToml: string;
  permissionMode: PermissionMode;
  configExists: boolean;
};

export type StudioAction =
  | { type: "bootstrapLoaded"; payload: BootstrapPayload; status: string }
  | { type: "bootstrapFailed"; status: string }
  | { type: "timelineLoaded"; sessionId: string | null; items: TimelineItem[]; nextSequence: number }
  | { type: "timelineLoadFailed"; sessionId: string | null; status: string }
  | { type: "projectSelectionLoaded"; payload: ProjectSelectionPayload; status: string }
  | { type: "sessionSelectionLoaded"; payload: SessionSelectionPayload; status: string }
  | { type: "sessionModeUpdated"; payload: SessionSelectionPayload; status: string }
  | { type: "runPromptLoaded"; payload: RunPromptResponse; status: string }
  | { type: "runPromptFailed"; sessionId?: string | null; status: string }
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
  | {
      type: "agentEvent";
      sessionId: string;
      event?: AgentEvent | null;
      timelineEvent?: AgentTimelineEvent | null;
      agent?: AgentDto | null;
      sessionRuntime?: SessionRuntime | null;
      statusText: string;
    }
  | { type: "promptSubmitted"; status: string; startedAt: number };

export const initialStudioState = (startingStatus: string): StudioState => ({
  projects: [],
  sessions: [],
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
  agents: [],
  agentTimelineEvents: [],
  ...emptyTimelineState(),
  sessionRuntime: null,
  approvals: [],
  settingsOpen: false,
  activeSettingsTab: "providers",
  providerSearch: "",
  selectedProviderId: null,
  configToml: "",
  permissionMode: "request-approval",
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
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        ...emptyTimelineState(),
        ...configFields(state.selectedProviderId, action.payload.config),
        status: action.status,
        turnPhase: "idle",
      };
    case "bootstrapFailed":
      return { ...state, status: action.status };
    case "timelineLoaded":
      if (action.sessionId !== state.selectedSessionId) return state;
      return mergeTimelineSnapshot(state, action.sessionId, action.items ?? [], action.nextSequence);
    case "timelineLoadFailed":
      if (action.sessionId !== state.selectedSessionId) return state;
      return { ...state, status: action.status };
    case "projectSelectionLoaded":
      return {
        ...state,
        projects: action.payload.projects,
        selectedProjectId: action.payload.projectId,
        sessions: action.payload.sessions,
        selectedSessionId: action.payload.selectedSessionId ?? null,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        ...emptyTimelineState(),
        approvals: [],
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
        isBusy: false,
      };
    case "sessionSelectionLoaded":
      return {
        ...state,
        sessions: action.payload.sessions.length > 0 ? action.payload.sessions : state.sessions,
        selectedSessionId: action.payload.sessionId,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        ...emptyTimelineState(),
        approvals: [],
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
        isBusy: false,
      };
    case "sessionModeUpdated":
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
      return {
        ...state,
        sessions: action.payload.sessions.length > 0 ? action.payload.sessions : state.sessions,
        selectedSessionId: action.payload.sessionId,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? state.sessionRuntime,
        status: action.status,
      };
    case "runPromptLoaded":
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
      return {
        ...mergeRunPromptTimeline(
          removeOptimisticTimelineItems(state),
          action.payload.sessionId,
          action.payload.timelineItems ?? [],
          action.payload.timelineNextSequence,
        ),
        selectedSessionId: action.payload.sessionId,
        sessions: action.payload.sessions,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime,
        turnPhase: phaseForTurnStatus(action.payload.turnStatus),
        turnStartedAt: null,
        status: action.status,
        isBusy: false,
      };
    case "runPromptFailed":
      if (action.sessionId && action.sessionId !== state.selectedSessionId) {
        return state;
      }
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
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
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
    case "promptSubmitted":
      return {
        ...removeOptimisticTimelineItems(state),
        prompt: "",
        isBusy: true,
        status: action.status,
        turnPhase: "running",
        turnStartedAt: action.startedAt,
      };
    case "agentEvent":
      if (action.sessionId !== state.selectedSessionId) {
        return state;
      }
      const nextRuntime = mergeLiveSkillIntoRuntime(
        action.sessionRuntime ?? state.sessionRuntime,
        action.event,
      );
      return reduceAgentEvent(
        {
          ...state,
          agentTimelineEvents: action.timelineEvent
            ? mergeAgentTimelineEvents(state.agentTimelineEvents, [action.timelineEvent])
            : state.agentTimelineEvents,
          agents: action.agent ? mergeAgents(state.agents, [action.agent]) : state.agents,
          sessionRuntime: nextRuntime,
        },
        action.event,
        action.statusText,
      );
    default:
      return state;
  }
}

function reduceAgentEvent(
  state: StudioState,
  event: AgentEvent | null | undefined,
  statusText: string,
): StudioState {
  if (!event) {
    return {
      ...state,
      status: statusText,
      turnPhase: state.turnPhase === "idle" ? "subagent" : state.turnPhase,
    };
  }
  if (event === "done") {
    return {
      ...state,
      status: statusText,
      turnPhase:
        state.turnPhase === "interrupted" ||
        state.turnPhase === "failed" ||
        state.turnPhase === "budgetLimited"
          ? state.turnPhase
          : "completed",
      turnStartedAt: null,
    };
  }
  if ("turnInterrupted" in event) {
    return { ...state, status: statusText, turnPhase: "interrupted" };
  }
  if ("turnBudgetLimited" in event) {
    return { ...state, status: statusText, turnPhase: "budgetLimited" };
  }
  if ("timelineItemStarted" in event) {
    const timelineState = applyLiveTimelineEvent(state, state.selectedSessionId ?? "", event);
    return {
      ...timelineState,
      status: statusText,
      turnPhase: phaseForTimelineItem(event.timelineItemStarted.item, state.turnPhase),
      turnStartedAt: state.turnStartedAt ?? Date.now(),
    };
  }
  if ("timelineItemDelta" in event) {
    const timelineState = applyLiveTimelineEvent(state, state.selectedSessionId ?? "", event);
    return {
      ...timelineState,
      status: statusText,
      turnPhase: phaseForTimelineDelta(event.timelineItemDelta.event, state.turnPhase),
    };
  }
  if ("timelineItemCompleted" in event) {
    const nextState = applyLiveTimelineEvent(state, state.selectedSessionId ?? "", event);
    return {
      ...nextState,
      status: statusText,
      turnPhase: phaseForTimelineItem(event.timelineItemCompleted.item, state.turnPhase),
    };
  }
  if ("timelineItemFailed" in event) {
    return {
      ...applyLiveTimelineEvent(state, state.selectedSessionId ?? "", event),
      status: statusText,
      turnPhase:
        event.timelineItemFailed.item.status === "budgetLimited"
          ? "budgetLimited"
          : event.timelineItemFailed.item.status === "interrupted"
            ? "interrupted"
            : "failed",
    };
  }
  if ("error" in event) {
    return { ...state, status: statusText, turnPhase: "failed" };
  }
  return { ...state, status: statusText };
}

function mergeLiveSkillIntoRuntime(
  runtime: SessionRuntime | null,
  event: AgentEvent | null | undefined,
): SessionRuntime | null {
  if (!runtime || !isRecord(event) || !("timelineItemCompleted" in event)) {
    return runtime;
  }
  const completed = event.timelineItemCompleted;
  if (!isRecord(completed)) {
    return runtime;
  }
  const skillName = skillNameFromCompletedItem(completed.item as TimelineItem);
  if (!skillName) {
    return runtime;
  }
  const activeSkills: string[] = [];
  const seen = new Set<string>();
  for (const name of [...runtime.activeSkills, skillName]) {
    const key = name.toLowerCase();
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    activeSkills.push(name);
  }
  if (
    activeSkills.length === runtime.activeSkills.length &&
    activeSkills.every((name, index) => name === runtime.activeSkills[index])
  ) {
    return runtime;
  }
  return {
    ...runtime,
    activeSkills,
  };
}

function skillNameFromCompletedItem(item: TimelineItem): string | null {
  if (item.kind !== "tool" || item.tool?.name !== "skill_view" || !item.tool.result) {
    return null;
  }
  let value: unknown;
  try {
    value = JSON.parse(item.tool.result);
  } catch {
    return null;
  }
  if (!isRecord(value) || value.success !== true || !isRecord(value.skill)) {
    return null;
  }
  const name = value.skill.name;
  return typeof name === "string" && name.trim() ? name.trim() : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function mergeAgentTimelineEvents(
  current: AgentTimelineEvent[],
  incoming: AgentTimelineEvent[],
): AgentTimelineEvent[] {
  if (incoming.length === 0) return current;
  const byId = new Map(current.map((event) => [event.eventId, event]));
  for (const event of incoming) {
    byId.set(event.eventId, event);
  }
  return [...byId.values()].sort((left, right) => {
    if (left.sequence !== right.sequence) return left.sequence - right.sequence;
    return left.eventId.localeCompare(right.eventId);
  });
}

export function mergeAgents(current: AgentDto[], incoming: AgentDto[]): AgentDto[] {
  if (incoming.length === 0) return current;
  const byId = new Map(current.map((agent) => [agent.id, agent]));
  for (const agent of incoming) {
    const existing = byId.get(agent.id);
    byId.set(agent.id, {
      ...agent,
      runtimeUsage: agent.runtimeUsage ?? existing?.runtimeUsage ?? null,
    });
  }
  return [...byId.values()].sort((left, right) => {
    if (left.path !== right.path) {
      return left.path.localeCompare(right.path);
    }
    return right.updatedAt - left.updatedAt;
  });
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
    permissionMode: payload.permissionMode,
    configExists: payload.configExists,
    selectedProviderId: nextProviderId,
  };
}

function phaseForTimelineItem(item: TimelineItem, current: TurnPhase): TurnPhase {
  if (current === "stopping") return "stopping";
  if (item.kind === "thinking") return "thinking";
  if (item.kind === "tool") return "tool";
  if (item.kind === "agent") return "subagent";
  if (item.kind === "plan") return current === "idle" ? "running" : current;
  if (item.status === "failed") return "failed";
  if (item.status === "interrupted") return "interrupted";
  if (item.status === "budgetLimited") return "budgetLimited";
  if (item.kind === "turn" && item.status === "completed") return "completed";
  return "running";
}

function phaseForTimelineDelta(event: TimelineItemDeltaEvent, current: TurnPhase): TurnPhase {
  if (current === "stopping") return "stopping";
  if (event.kind === "thinking") return "thinking";
  if (event.kind === "tool") return "tool";
  if (event.kind === "plan") return current === "idle" ? "running" : current;
  return "running";
}

export function phaseForTurnStatus(status: TurnStatus): TurnPhase {
  switch (status) {
    case "started":
      return "running";
    case "completed":
      return "completed";
    case "errored":
      return "failed";
    case "aborted":
      return "interrupted";
  }
}
