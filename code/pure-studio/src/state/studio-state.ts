import type {
  AgentDto,
  AgentEvent,
  AgentTimelineEvent,
  BootstrapPayload,
  ConfigPayload,
  InstructionsRecord,
  LspHealthUpdatedPayload,
  LspServerRecord,
  McpHealthUpdatedPayload,
  McpServerRecord,
  PermissionMode,
  PlanState,
  ProjectRecord,
  ProjectSelectionPayload,
  ProviderRecord,
  ProviderUsageRecord,
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
  InteractionChangedPayload,
  InteractionRequest,
} from "../types";
import {
  applyLiveTimelineEvent,
  emptyTimelineState,
  mergeRunPromptTimeline,
  mergeTimelineSnapshot,
  removeOptimisticTimelineItems,
  removeOptimisticUserTimelineItems,
  removeOptimisticWaitingTimelineItems,
  type TimelineStateSlice,
} from "./timeline-state";

export type SettingsTab =
  | "providers"
  | "instructions"
  | "skills"
  | "roles"
  | "mcp"
  | "security"
  | "general";

export type StudioState = TimelineStateSlice & {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  providers: ProviderRecord[];
  providerUsages: ProviderUsageRecord[];
  providerUsagesLoading: boolean;
  providerUsageError: string | null;
  mcpServers: McpServerRecord[];
  lspServers: LspServerRecord[];
  roles: RoleRecord[];
  providerTemplates: ProviderTemplateRecord[];
  instructions: InstructionsRecord;
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
  interactions: Map<string, InteractionRequest>;
  activeInteractionId: string | null;
  planStates: Map<string, PlanState>;
  dismissedPlanId: string | null;
  currentRunTimelineBaseSequence: number | null;
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
  | {
      type: "timelineLoaded";
      sessionId: string | null;
      items: TimelineItem[];
      planStates?: PlanState[];
      interactions?: InteractionRequest[];
      nextSequence: number;
    }
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
  | { type: "providerUsagesLoading" }
  | { type: "providerUsagesLoaded"; usages: ProviderUsageRecord[] }
  | { type: "providerUsagesFailed"; error: string }
  | { type: "setConfigToml"; toml: string }
  | { type: "setSettingsOpen"; value: boolean; tab?: SettingsTab }
  | { type: "configLoaded"; payload: ConfigPayload; status?: string }
  | { type: "mcpHealthUpdated"; payload: McpHealthUpdatedPayload }
  | { type: "lspHealthUpdated"; payload: LspHealthUpdatedPayload }
  | { type: "interactionChanged"; payload: InteractionChangedPayload; status?: string }
  | {
      type: "planLifecycleLoaded";
      sessionId: string;
      planStates: PlanState[];
      timelineNextSequence: number;
      status?: string;
    }
  | { type: "stopRequested"; status: string }
  | { type: "stopFallback"; status: string }
  | { type: "planImplementationSubmitted"; status: string; startedAt: number }
  | {
      type: "agentEvent";
      sessionId: string;
      event?: AgentEvent | null;
      timelineEvent?: AgentTimelineEvent | null;
      agent?: AgentDto | null;
      sessionRuntime?: SessionRuntime | null;
      statusText: string;
    }
  | { type: "promptSubmitted"; status: string; startedAt: number; prompt: string };

export const initialStudioState = (startingStatus: string): StudioState => ({
  projects: [],
  sessions: [],
  providers: [],
  providerUsages: [],
  providerUsagesLoading: false,
  providerUsageError: null,
  mcpServers: [],
  lspServers: [],
  roles: [],
  providerTemplates: [],
  instructions: {
    baseOverride: "",
    developer: "",
    user: "",
    projectDocMaxBytes: 65536,
    projectDocFallbackFilenames: [],
  },
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
  interactions: new Map(),
  activeInteractionId: null,
  planStates: new Map(),
  dismissedPlanId: null,
  currentRunTimelineBaseSequence: null,
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
        sessions: sessionList(action.payload.sessions),
        selectedSessionId: action.payload.selectedSessionId ?? null,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        lspServers: action.payload.lspHealth?.lspServers ?? state.lspServers,
        ...emptyTimelineState(),
        ...configFields(state.selectedProviderId, action.payload.config),
        status: action.status,
        turnPhase: "idle",
        ...interactionState(
          new Map(),
          action.payload.interactions ?? [],
          action.payload.selectedSessionId ?? null,
          false,
        ),
        planStates: new Map(),
        dismissedPlanId: null,
        currentRunTimelineBaseSequence: null,
      };
    case "bootstrapFailed":
      return { ...state, status: action.status };
    case "timelineLoaded":
      if (action.sessionId !== state.selectedSessionId) return state;
      return {
        ...mergeTimelineSnapshot(state, action.sessionId, action.items ?? [], action.nextSequence),
        planStates: mergePlanStates(state.planStates, action.planStates ?? []),
        ...interactionState(
          state.interactions,
          action.interactions ?? [],
          state.selectedSessionId,
          state.isBusy,
        ),
      };
    case "timelineLoadFailed":
      if (action.sessionId !== state.selectedSessionId) return state;
      return { ...state, status: action.status };
    case "projectSelectionLoaded":
      const selectedProjectId = action.payload.selectedProjectId ?? action.payload.projectId ?? null;
      return {
        ...state,
        projects: action.payload.projects,
        selectedProjectId,
        sessions: sessionList(action.payload.sessions),
        selectedSessionId: action.payload.selectedSessionId ?? null,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        lspServers: action.payload.lspHealth?.lspServers ?? state.lspServers,
        ...emptyTimelineState(),
        ...interactionState(
          new Map(),
          action.payload.interactions ?? [],
          action.payload.selectedSessionId ?? null,
          false,
        ),
        planStates: new Map(),
        dismissedPlanId: null,
        currentRunTimelineBaseSequence: null,
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
        isBusy: false,
      };
    case "sessionSelectionLoaded":
      if (action.payload.sessionId === state.selectedSessionId) {
        return {
          ...state,
          sessions: sessionListOrPrevious(action.payload.sessions, state.sessions),
          agentTimelineEvents: mergeAgentTimelineEvents(
            state.agentTimelineEvents,
            action.payload.agentEvents ?? [],
          ),
          agents: mergeAgents(state.agents, action.payload.agents ?? []),
          sessionRuntime: action.payload.sessionRuntime ?? state.sessionRuntime,
          ...interactionState(
            state.interactions,
            action.payload.interactions ?? [],
            state.selectedSessionId,
            state.isBusy,
          ),
          status: action.status,
        };
      }
      return {
        ...state,
        sessions: sessionListOrPrevious(action.payload.sessions, state.sessions),
        selectedSessionId: action.payload.sessionId,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? null,
        ...emptyTimelineState(),
        ...interactionState(new Map(), action.payload.interactions ?? [], action.payload.sessionId, false),
        planStates: new Map(),
        dismissedPlanId: null,
        currentRunTimelineBaseSequence: null,
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
        sessions: sessionListOrPrevious(action.payload.sessions, state.sessions),
        selectedSessionId: action.payload.sessionId,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? state.sessionRuntime,
        status: action.status,
      };
    case "runPromptLoaded": {
      const switchingSession = action.payload.sessionId !== state.selectedSessionId;
      const responseContainsSelectedSession = action.payload.sessions.some(
        (session) => session.id === action.payload.sessionId,
      );
      if (switchingSession && !state.isBusy && !responseContainsSelectedSession) {
        return state;
      }
      const timelineBase = switchingSession
        ? {
            ...state,
            ...emptyTimelineState(),
            planStates: new Map<string, PlanState>(),
            interactions: new Map<string, InteractionRequest>(),
          }
        : removeOptimisticTimelineItems(state);
      return {
        ...mergeRunPromptTimeline(
          timelineBase,
          action.payload.sessionId,
          action.payload.timelineItems ?? [],
          action.payload.timelineNextSequence,
        ),
        planStates: mergePlanStates(timelineBase.planStates, action.payload.planStates ?? []),
        selectedSessionId: action.payload.sessionId,
        sessions: sessionList(action.payload.sessions),
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime,
        turnPhase: phaseForTurnStatus(action.payload.turnStatus),
        turnStartedAt: null,
        status: action.status,
        isBusy: false,
        ...interactionState(
          timelineBase.interactions,
          action.payload.interactions ?? [],
          action.payload.sessionId,
          false,
        ),
        currentRunTimelineBaseSequence: null,
      };
    }
    case "runPromptFailed":
      if (action.sessionId && action.sessionId !== state.selectedSessionId) {
        return state;
      }
      return {
        ...removeOptimisticWaitingTimelineItems(state),
        status: action.status,
        turnPhase: "failed",
        turnStartedAt: null,
        isBusy: false,
        ...clearSessionInteractions(state.interactions, state.selectedSessionId),
        currentRunTimelineBaseSequence: null,
      };
    case "setBusy":
      return {
        ...state,
        isBusy: action.value,
        activeInteractionId: selectActiveInteractionId(
          state.interactions,
          state.selectedSessionId,
          action.value,
        ),
      };
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
    case "providerUsagesLoading":
      return { ...state, providerUsagesLoading: true, providerUsageError: null };
    case "providerUsagesLoaded":
      return {
        ...state,
        providerUsages: mergeProviderUsages(state.providerUsages, action.usages),
        providerUsagesLoading: false,
        providerUsageError: null,
      };
    case "providerUsagesFailed":
      return {
        ...state,
        providerUsagesLoading: false,
        providerUsageError: action.error,
      };
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
    case "mcpHealthUpdated":
      return {
        ...state,
        mcpServers: mergeMcpServers(state.mcpServers, action.payload.mcpServers),
        sessionRuntime: state.sessionRuntime
          ? {
              ...state.sessionRuntime,
              activeMcpServers: action.payload.activeMcpServers,
            }
          : state.sessionRuntime,
      };
    case "lspHealthUpdated":
      return {
        ...state,
        lspServers: action.payload.lspServers,
        sessionRuntime: state.sessionRuntime
          ? {
              ...state.sessionRuntime,
              activeLspServers: action.payload.activeLspServers,
            }
          : state.sessionRuntime,
      };
    case "interactionChanged":
      return reduceInteractionChanged(state, action.payload, action.status);
    case "planLifecycleLoaded":
      if (action.sessionId !== state.selectedSessionId) {
        return state;
      }
      return {
        ...state,
        planStates: mergePlanStates(state.planStates, action.planStates ?? []),
        timelineNextSequence: Math.max(state.timelineNextSequence, action.timelineNextSequence),
        status: action.status ?? state.status,
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
        ...clearSessionInteractions(state.interactions, state.selectedSessionId),
        currentRunTimelineBaseSequence: null,
      };
    case "planImplementationSubmitted":
      return appendOptimisticWaiting(
        {
          ...state,
          isBusy: true,
          status: action.status,
          turnPhase: "running",
          turnStartedAt: action.startedAt,
          ...clearSessionInteractions(state.interactions, state.selectedSessionId),
        },
        action.startedAt,
      );
    case "promptSubmitted":
      const currentRunTimelineBaseSequence = state.timelineNextSequence;
      return {
        ...appendOptimisticPrompt(removeOptimisticTimelineItems(state), action.prompt, action.startedAt),
        prompt: "",
        isBusy: true,
        status: action.status,
        turnPhase: "running",
        turnStartedAt: action.startedAt,
        currentRunTimelineBaseSequence,
        ...clearSessionInteractions(state.interactions, state.selectedSessionId),
      };
    case "agentEvent":
      if (action.sessionId !== state.selectedSessionId) {
        return state;
      }
      const eventBase = applyOptimisticTimelineCleanup(state, action.event);
      return reduceAgentEvent(
        {
          ...eventBase,
          agentTimelineEvents: action.timelineEvent
            ? mergeAgentTimelineEvents(eventBase.agentTimelineEvents, [action.timelineEvent])
            : eventBase.agentTimelineEvents,
          agents: action.agent ? mergeAgents(eventBase.agents, [action.agent]) : eventBase.agents,
          sessionRuntime: action.sessionRuntime ?? eventBase.sessionRuntime,
        },
        action.event,
        action.statusText,
      );
    default:
      return state;
  }
}

function interactionState(
  current: Map<string, InteractionRequest>,
  incoming: InteractionRequest[],
  selectedSessionId: string | null,
  isBusy = false,
): Pick<StudioState, "interactions" | "activeInteractionId"> {
  const interactions = new Map(current);
  for (const interaction of incoming) {
    if (!interaction?.interactionId) {
      continue;
    }
    interactions.set(interaction.interactionId, interaction);
  }
  return {
    interactions,
    activeInteractionId: selectActiveInteractionId(interactions, selectedSessionId, isBusy),
  };
}

function reduceInteractionChanged(
  state: StudioState,
  payload: InteractionChangedPayload,
  status?: string,
): StudioState {
  const interaction = payload.event.interaction;
  if (interaction.scope.sessionId !== state.selectedSessionId) {
    return state;
  }
  const interactions = new Map(state.interactions);
  interactions.set(interaction.interactionId, interaction);
  return {
    ...state,
    interactions,
    activeInteractionId: selectActiveInteractionId(interactions, state.selectedSessionId, state.isBusy),
    status: status ?? state.status,
    turnPhase: phaseForInteraction(interaction, state.turnPhase),
  };
}

function clearSessionInteractions(
  current: Map<string, InteractionRequest>,
  selectedSessionId: string | null,
): Pick<StudioState, "interactions" | "activeInteractionId"> {
  if (!selectedSessionId) {
    return { interactions: current, activeInteractionId: null };
  }
  const interactions = new Map(current);
  for (const [id, interaction] of interactions) {
    if (interaction.scope.sessionId === selectedSessionId && interaction.status === "pending") {
      interactions.delete(id);
    }
  }
  return {
    interactions,
    activeInteractionId: selectActiveInteractionId(interactions, selectedSessionId, false),
  };
}

export function selectActiveInteractionId(
  interactions: Map<string, InteractionRequest>,
  selectedSessionId: string | null,
  isBusy: boolean,
): string | null {
  if (!selectedSessionId) {
    return null;
  }
  const pending = [...interactions.values()].filter(
    (interaction) =>
      interaction.scope.sessionId === selectedSessionId &&
      interaction.status === "pending" &&
      (!isBusy || interaction.kind !== "planConfirmation"),
  );
  pending.sort((left, right) => {
    const priorityDelta = interactionPriority(right) - interactionPriority(left);
    if (priorityDelta !== 0) {
      return priorityDelta;
    }
    if (right.updatedAt !== left.updatedAt) {
      return right.updatedAt - left.updatedAt;
    }
    return right.interactionId.localeCompare(left.interactionId);
  });
  return pending[0]?.interactionId ?? null;
}

function interactionPriority(interaction: InteractionRequest): number {
  switch (interaction.kind) {
    case "toolApproval":
      return 3;
    case "userInput":
      return 2;
    case "planConfirmation":
      return 1;
  }
}

function phaseForInteraction(interaction: InteractionRequest, current: TurnPhase): TurnPhase {
  if (interaction.status !== "pending") {
    if (interaction.kind === "planConfirmation") {
      return current === "stopping" ? "stopping" : "idle";
    }
    return current === "stopping" ? "stopping" : "tool";
  }
  switch (interaction.kind) {
    case "toolApproval":
      return "approval";
    case "userInput":
      return "userInput";
    case "planConfirmation":
      return current;
  }
}

function mergePlanStates(
  current: Map<string, PlanState>,
  incoming: PlanState[],
): Map<string, PlanState> {
  if (!incoming.length) {
    return current;
  }
  const next = new Map(current);
  for (const planState of incoming) {
    if (!planState?.planId) {
      continue;
    }
    next.set(planState.planId, planState);
  }
  return next;
}

function sessionList(sessions: SessionRecord[]): SessionRecord[] {
  const byId = new Map<string, SessionRecord>();
  for (const session of sessions) {
    if (!session?.id || byId.has(session.id)) {
      continue;
    }
    byId.set(session.id, session);
  }
  return [...byId.values()];
}

function sessionListOrPrevious(
  sessions: SessionRecord[],
  previous: SessionRecord[],
): SessionRecord[] {
  return sessions.length > 0 ? sessionList(sessions) : previous;
}

function appendOptimisticPrompt(state: StudioState, prompt: string, startedAt: number): StudioState {
  const content = prompt.trim();
  if (!content) {
    return state;
  }
  const createdAt = Math.floor(startedAt / 1000);
  const turnId = `optimistic-turn-${startedAt}`;
  const nextSequence = state.timelineNextSequence;
  const userItem: TimelineItem = {
    turnId,
    itemId: `optimistic-user-${startedAt}`,
    sequence: nextSequence,
    kind: "text",
    status: "completed",
    createdAt,
    updatedAt: createdAt,
    role: "user",
    content,
    thinkingChunks: [],
  };
  const waitingItem: TimelineItem = {
    turnId,
    itemId: `optimistic-waiting-${startedAt}`,
    sequence: nextSequence + 1,
    kind: "turn",
    status: "running",
    createdAt,
    updatedAt: createdAt,
    role: null,
    content: "waitingForModel",
    thinkingChunks: [],
  };
  const timelineItems = new Map(state.timelineItems);
  timelineItems.set(userItem.itemId, userItem);
  timelineItems.set(waitingItem.itemId, waitingItem);
  return {
    ...state,
    timelineItems,
    timelineOrder: [...state.timelineOrder, userItem.itemId, waitingItem.itemId],
  };
}

function appendOptimisticWaiting(state: StudioState, startedAt: number): StudioState {
  const createdAt = Math.floor(startedAt / 1000);
  const turnId = `optimistic-turn-${startedAt}`;
  const waitingItem: TimelineItem = {
    turnId,
    itemId: `optimistic-waiting-${startedAt}`,
    sequence: state.timelineNextSequence,
    kind: "turn",
    status: "running",
    createdAt,
    updatedAt: createdAt,
    role: null,
    content: "waitingForModel",
    thinkingChunks: [],
  };
  const timelineItems = new Map(state.timelineItems);
  timelineItems.set(waitingItem.itemId, waitingItem);
  return {
    ...state,
    timelineItems,
    timelineOrder: [...state.timelineOrder, waitingItem.itemId],
  };
}

function applyOptimisticTimelineCleanup(
  state: StudioState,
  event: AgentEvent | null | undefined,
): StudioState {
  let next = state;
  if (shouldClearOptimisticUserTimeline(event)) {
    next = removeOptimisticUserTimelineItems(next);
  }
  if (shouldClearOptimisticWaitingTimeline(event)) {
    next = removeOptimisticWaitingTimelineItems(next);
  }
  return next;
}

function shouldClearOptimisticUserTimeline(event: AgentEvent | null | undefined): boolean {
  if (!isRecord(event)) {
    return false;
  }
  const item = timelineEventItem(event);
  return item?.kind === "text" && item.role === "user";
}

function shouldClearOptimisticWaitingTimeline(event: AgentEvent | null | undefined): boolean {
  if (event === "done") {
    return true;
  }
  if (!isRecord(event)) {
    return false;
  }
  if ("timelineItemDelta" in event && isRecord(event.timelineItemDelta)) {
    const timelineEvent = event.timelineItemDelta.event as TimelineItemDeltaEvent | undefined;
    return timelineEvent ? isModelVisibleTimelineKind(timelineEvent.kind) : false;
  }
  const item = timelineEventItem(event);
  if (!item) {
    return "turnInterrupted" in event || "turnBudgetLimited" in event || "error" in event;
  }
  if (isModelVisibleTimelineItem(item)) {
    return true;
  }
  return item.kind === "turn" && isTerminalTimelineStatus(item.status);
}

function timelineEventItem(event: Record<string, unknown>): TimelineItem | null {
  if ("timelineItemStarted" in event && isRecord(event.timelineItemStarted)) {
    return event.timelineItemStarted.item as TimelineItem;
  }
  if ("timelineItemCompleted" in event && isRecord(event.timelineItemCompleted)) {
    return event.timelineItemCompleted.item as TimelineItem;
  }
  if ("timelineItemFailed" in event && isRecord(event.timelineItemFailed)) {
    return event.timelineItemFailed.item as TimelineItem;
  }
  return null;
}

function isModelVisibleTimelineItem(item: TimelineItem): boolean {
  if (item.kind === "text") {
    return item.role === "assistant";
  }
  return isModelVisibleTimelineKind(item.kind) || item.kind === "inference" && item.status === "completed";
}

function isModelVisibleTimelineKind(kind: TimelineItem["kind"]): boolean {
  switch (kind) {
    case "text":
    case "thinking":
    case "tool":
    case "agent":
    case "plan":
      return true;
    case "turn":
    case "inference":
      return false;
  }
}

function isTerminalTimelineStatus(status: TimelineItem["status"]): boolean {
  switch (status) {
    case "completed":
    case "failed":
    case "denied":
    case "interrupted":
    case "budgetLimited":
      return true;
    case "started":
    case "streaming":
    case "awaitingApproval":
    case "approved":
    case "running":
      return false;
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
  if ("interactionChanged" in event) {
    return reduceInteractionChanged(
      state,
      {
        sessionId: state.selectedSessionId ?? "",
        event: event.interactionChanged.event,
      },
      statusText,
    );
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
    const completed = event.timelineItemCompleted.item;
    return {
      ...applyLiveTimelineEvent(state, state.selectedSessionId ?? "", event),
      status: statusText,
      turnPhase: phaseForTimelineItem(completed, state.turnPhase),
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
    mcpServers: payload.mcpServers ?? [],
    roles: payload.roles,
    instructions: payload.instructions,
    providerTemplates: payload.templates,
    configToml: payload.toml,
    permissionMode: payload.permissionMode,
    configExists: payload.configExists,
    selectedProviderId: nextProviderId,
  };
}

function mergeMcpServers(
  current: McpServerRecord[],
  incoming: McpServerRecord[],
): McpServerRecord[] {
  if (incoming.length === 0) return current;
  const byId = new Map(current.map((server) => [server.id, server]));
  return incoming.map((server) => {
    const existing = byId.get(server.id);
    return {
      ...existing,
      ...server,
    };
  });
}

function mergeProviderUsages(
  current: ProviderUsageRecord[],
  incoming: ProviderUsageRecord[],
): ProviderUsageRecord[] {
  const byId = new Map(current.map((usage) => [usage.providerId, usage]));
  for (const usage of incoming) {
    byId.set(usage.providerId, usage);
  }
  return [...byId.values()];
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
