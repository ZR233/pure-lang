import type {
  AgentDto,
  AgentEvent,
  AgentTimelineEvent,
  BootstrapPayload,
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
  TimelineItemDeltaEvent,
  TurnPhase,
  TurnStatus,
  ToolApprovalRequest,
  ToolApprovalResolved,
} from "../types";

export type SettingsTab = "providers" | "skills" | "roles" | "security" | "general";

export type StudioState = {
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
  timelineItems: Map<string, TimelineItem>;
  timelineOrder: string[];
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
  | { type: "timelineLoaded"; sessionId: string | null; items: TimelineItem[] }
  | { type: "timelineLoadFailed"; sessionId: string | null; status: string }
  | { type: "projectSelectionLoaded"; payload: ProjectSelectionPayload; status: string }
  | { type: "sessionSelectionLoaded"; payload: SessionSelectionPayload; status: string }
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
  timelineItems: new Map(),
  timelineOrder: [],
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
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        ...configFields(state.selectedProviderId, action.payload.config),
        status: action.status,
        turnPhase: "idle",
      };
    case "bootstrapFailed":
      return { ...state, status: action.status };
    case "timelineLoaded":
      if (action.sessionId !== state.selectedSessionId) return state;
      return replaceTimelineItems(state, action.items ?? []);
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
        timelineItems: new Map(),
        timelineOrder: [],
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
        timelineItems: new Map(),
        timelineOrder: [],
        approvals: [],
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
        isBusy: false,
      };
    case "runPromptLoaded":
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
      return {
        ...mergeTimelineItems(
          removeOptimisticTimelineItems(state),
          action.payload.timelineItems ?? [],
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
      return reduceAgentEvent(
        {
          ...state,
          agentTimelineEvents: action.timelineEvent
            ? mergeAgentTimelineEvents(state.agentTimelineEvents, [action.timelineEvent])
            : state.agentTimelineEvents,
          agents: action.agent ? mergeAgents(state.agents, [action.agent]) : state.agents,
          sessionRuntime: action.sessionRuntime ?? state.sessionRuntime,
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
    return {
      ...upsertTimelineItem(state, event.timelineItemStarted.item),
      status: statusText,
      turnPhase: phaseForTimelineItem(event.timelineItemStarted.item, state.turnPhase),
      turnStartedAt: state.turnStartedAt ?? Date.now(),
    };
  }
  if ("timelineItemDelta" in event) {
    return {
      ...applyTimelineDelta(state, event.timelineItemDelta.event),
      status: statusText,
      turnPhase: phaseForTimelineDelta(event.timelineItemDelta.event, state.turnPhase),
    };
  }
  if ("timelineItemCompleted" in event) {
    const nextState = upsertTimelineItem(state, event.timelineItemCompleted.item);
    return {
      ...nextState,
      status: statusText,
      turnPhase: phaseForTimelineItem(event.timelineItemCompleted.item, state.turnPhase),
    };
  }
  if ("timelineItemFailed" in event) {
    return {
      ...upsertTimelineItem(state, event.timelineItemFailed.item),
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

function replaceTimelineItems(state: StudioState, items: TimelineItem[]): StudioState {
  const next = {
    ...state,
    timelineItems: new Map<string, TimelineItem>(),
    timelineOrder: [],
  };
  return mergeTimelineItems(next, items);
}

function removeOptimisticTimelineItems(state: StudioState): StudioState {
  const timelineItems = new Map(state.timelineItems);
  for (const itemId of state.timelineOrder) {
    if (itemId.startsWith("optimistic-")) {
      timelineItems.delete(itemId);
    }
  }
  return {
    ...state,
    timelineItems,
    timelineOrder: state.timelineOrder.filter((itemId) => !itemId.startsWith("optimistic-")),
  };
}

export function mergeTimelineItems(state: StudioState, incoming: TimelineItem[]): StudioState {
  let next = state;
  for (const item of incoming ?? []) {
    if (!item?.itemId) continue;
    next = upsertTimelineItem(next, item);
  }
  return next;
}

function upsertTimelineItem(state: StudioState, item: TimelineItem): StudioState {
  if (!item?.itemId) {
    return state;
  }
  const timelineItems = new Map(state.timelineItems);
  const existing = timelineItems.get(item.itemId);
  timelineItems.set(item.itemId, mergeTimelineItem(existing, item));
  const timelineOrder = existing
    ? state.timelineOrder
    : [...state.timelineOrder, item.itemId].sort((left, right) => {
        const leftItem = timelineItems.get(left);
        const rightItem = timelineItems.get(right);
        return (leftItem?.sequence ?? 0) - (rightItem?.sequence ?? 0);
      });
  return { ...state, timelineItems, timelineOrder };
}

function mergeTimelineItem(existing: TimelineItem | undefined, item: TimelineItem): TimelineItem {
  const incoming = normalizeTimelineItem(item);
  if (!existing) {
    return incoming;
  }
  const current = normalizeTimelineItem(existing);
  return {
    ...current,
    ...incoming,
    content: incoming.content || current.content || "",
    thinkingChunks:
      incoming.thinkingChunks.length > 0 ? incoming.thinkingChunks : current.thinkingChunks,
    tool: incoming.tool ?? current.tool ?? null,
    agent: incoming.agent ?? current.agent ?? null,
    inference: incoming.inference ?? current.inference ?? null,
    usage: incoming.usage ?? current.usage ?? null,
    sequence: current.sequence,
    createdAt: current.createdAt,
  };
}

function applyTimelineDelta(state: StudioState, event: TimelineItemDeltaEvent): StudioState {
  const existing = state.timelineItems.get(event.itemId) ?? blankTimelineItem(event);
  const item = normalizeTimelineItem(existing);
  item.status = event.status;
  item.updatedAt = event.updatedAt;
  const delta = event.delta;
  switch (delta.type) {
    case "text":
      item.content += delta.delta;
      break;
    case "thinking": {
      const chunk = item.thinkingChunks.find((part) => part.chunkIndex === delta.chunkIndex);
      if (chunk) {
        chunk.content += delta.delta;
      } else {
        item.thinkingChunks.push({
          chunkIndex: delta.chunkIndex,
          content: delta.delta,
        });
      }
      item.thinkingChunks.sort((left, right) => left.chunkIndex - right.chunkIndex);
      break;
    }
    case "toolArguments":
      if (item.tool) {
        item.tool.arguments += delta.delta;
      }
      break;
    case "toolResult":
      if (item.tool) {
        item.tool.result = `${item.tool.result ?? ""}${delta.delta}`;
      }
      break;
  }
  return upsertTimelineItem(state, item);
}

function blankTimelineItem(event: TimelineItemDeltaEvent): TimelineItem {
  return {
    turnId: event.turnId,
    itemId: event.itemId,
    sequence: event.sequence,
    kind: event.kind,
    status: event.status,
    createdAt: event.createdAt,
    updatedAt: event.updatedAt,
    role: null,
    content: "",
    thinkingChunks: [],
    tool: null,
    agent: null,
    inference: null,
    usage: null,
  };
}

function normalizeTimelineItem(item: TimelineItem): TimelineItem {
  return {
    ...item,
    content: item.content ?? "",
    thinkingChunks: item.thinkingChunks ?? [],
    tool: item.tool ?? null,
    agent: item.agent ?? null,
    inference: item.inference ?? null,
    usage: item.usage ?? null,
  };
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
    configExists: payload.configExists,
    selectedProviderId: nextProviderId,
  };
}

function phaseForTimelineItem(item: TimelineItem, current: TurnPhase): TurnPhase {
  if (current === "stopping") return "stopping";
  if (item.kind === "thinking") return "thinking";
  if (item.kind === "tool") return "tool";
  if (item.kind === "agent") return "subagent";
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
