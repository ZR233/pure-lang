import type {
  AgentDto,
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
  SessionRecord,
  SessionRuntime,
  SessionSelectionPayload,
  StudioAgentTimelineEvent,
  StudioEventEnvelope,
  StudioMessage,
  StudioMessageProjection,
  StudioPart,
  StudioPartProjection,
  StudioPartDeltaField,
  StudioTurnStatus,
  TimelinePartView,
  TimelineAttachment,
  TurnPhase,
  TurnStatus,
  InteractionChangedPayload,
  InteractionRequest,
} from "../types";
import {
  addOptimisticPart,
  applyStudioEvent,
  emptyTimelineState,
  mergeConversationSnapshot,
  removeOptimisticTimelinePartViews,
  removeOptimisticUserTimelinePartViews,
  removeOptimisticWaitingTimelinePartViews,
  resetConversationFromSnapshot,
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
  currentRunEventBaseSequence: number | null;
  settingsOpen: boolean;
  activeSettingsTab: SettingsTab;
  providerSearch: string;
  selectedProviderId: string | null;
  configToml: string;
  permissionMode: PermissionMode;
  configExists: boolean;
  sessionViews: Map<string, SessionViewState>;
};

type SessionViewState = TimelineStateSlice & {
  agentTimelineEvents: AgentTimelineEvent[];
  agents: AgentDto[];
  sessionRuntime: SessionRuntime | null;
  interactions: Map<string, InteractionRequest>;
  activeInteractionId: string | null;
  planStates: Map<string, PlanState>;
  dismissedPlanId: string | null;
  currentRunEventBaseSequence: number | null;
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
  isBusy: boolean;
};

export type StudioAction =
  | { type: "bootstrapLoaded"; payload: BootstrapPayload; status: string }
  | { type: "bootstrapFailed"; status: string }
  | {
      type: "sessionStateLoaded";
      sessionId: string | null;
      agentEvents?: AgentTimelineEvent[];
      agents?: AgentDto[];
      sessionRuntime?: SessionRuntime | null;
      messages?: StudioMessageProjection[];
      parts?: StudioPartProjection[];
      events: StudioEventEnvelope[];
      planStates?: PlanState[];
      interactions?: InteractionRequest[];
      nextSequence: number;
      status?: string;
    }
  | { type: "sessionStateLoadFailed"; sessionId: string | null; status: string }
  | { type: "projectSelectionLoaded"; payload: ProjectSelectionPayload; status: string }
  | { type: "sessionSelectionLoaded"; payload: SessionSelectionPayload; status: string }
  | { type: "sessionHandoffStarted"; payload: SessionSelectionPayload; status: string; startedAt: number }
  | { type: "sessionModeUpdated"; payload: SessionSelectionPayload; status: string }
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
  | { type: "studioEvent"; envelope: StudioEventEnvelope; status?: string }
  | {
      type: "planLifecycleLoaded";
      sessionId: string;
      planStates: PlanState[];
      eventNextSequence: number;
      status?: string;
    }
  | { type: "stopRequested"; status: string }
  | { type: "stopFallback"; status: string }
  | { type: "planImplementationSubmitted"; status: string; startedAt: number }
  | { type: "promptSubmitted"; status: string; startedAt: number; prompt: string; attachments?: TimelineAttachment[] };

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
  currentRunEventBaseSequence: null,
  settingsOpen: false,
  activeSettingsTab: "providers",
  providerSearch: "",
  selectedProviderId: null,
  configToml: "",
  permissionMode: "request-approval",
  configExists: false,
  sessionViews: new Map(),
});

export function studioReducer(state: StudioState, action: StudioAction): StudioState {
  switch (action.type) {
    case "bootstrapLoaded":
      return storeSessionView({
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
        currentRunEventBaseSequence: null,
      }, action.payload.selectedSessionId ?? null);
    case "bootstrapFailed":
      return { ...state, status: action.status };
    case "sessionStateLoaded":
      if (action.sessionId !== state.selectedSessionId) return state;
      {
        const staleSessionState = action.nextSequence < state.eventNextSequence;
        const conversationState = staleSessionState
          ? mergeConversationSnapshot(
              state,
              action.messages ?? [],
              action.parts ?? [],
              action.nextSequence,
            )
          : resetConversationFromSnapshot(
              state,
              action.messages ?? [],
              action.parts ?? [],
              action.nextSequence,
            );
        const next = reduceSessionStateLoaded(
          {
            ...conversationState,
            agentTimelineEvents: staleSessionState
              ? mergeAgentTimelineEvents(state.agentTimelineEvents, action.agentEvents ?? [])
              : action.agentEvents ?? state.agentTimelineEvents,
            agents: staleSessionState
              ? mergeAgents(state.agents, action.agents ?? [])
              : action.agents ?? state.agents,
            sessionRuntime: staleSessionState
              ? state.sessionRuntime
              : action.sessionRuntime ?? state.sessionRuntime,
            planStates: mergePlanStates(
              staleSessionState ? state.planStates : new Map(),
              action.planStates ?? [],
            ),
            ...interactionState(
              staleSessionState ? state.interactions : new Map(),
              action.interactions ?? [],
              state.selectedSessionId,
              state.isBusy,
            ),
            status: staleSessionState ? state.status : action.status ?? state.status,
          },
          freshSessionStateEvents(action.events ?? [], staleSessionState ? state.eventNextSequence : 0),
          action.status,
        );
        return storeSessionView(next, action.sessionId);
      }
    case "sessionStateLoadFailed":
      if (action.sessionId !== state.selectedSessionId) return state;
      return { ...state, status: action.status };
    case "projectSelectionLoaded":
      const selectedProjectId = action.payload.selectedProjectId ?? action.payload.projectId ?? null;
      return storeSessionView({
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
        currentRunEventBaseSequence: null,
        status: action.status,
        turnPhase: "idle",
        turnStartedAt: null,
        isBusy: false,
      }, action.payload.selectedSessionId ?? null);
    case "sessionSelectionLoaded":
      if (action.payload.sessionId === state.selectedSessionId) {
        const merged = {
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
        return storeSessionView(merged, action.payload.sessionId);
      }
      {
        const saved = storeSessionView(state);
        const fallback: SessionViewState = {
          ...emptySessionView(),
          agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
          agents: mergeAgents([], action.payload.agents ?? []),
          sessionRuntime: action.payload.sessionRuntime ?? null,
          ...interactionState(new Map(), action.payload.interactions ?? [], action.payload.sessionId, false),
        };
        return restoreSessionView(
          {
            ...saved,
            sessions: sessionListOrPrevious(action.payload.sessions, state.sessions),
            selectedSessionId: action.payload.sessionId,
            status: action.status,
            turnPhase: "idle",
            turnStartedAt: null,
            isBusy: false,
          },
          action.payload.sessionId,
          fallback,
        );
      }
    case "sessionHandoffStarted": {
      const saved = storeSessionView(state);
      const handoffState = {
        ...restoreSessionView(
          {
            ...saved,
            sessions: sessionListOrPrevious(action.payload.sessions, state.sessions),
            selectedSessionId: action.payload.sessionId,
            status: action.status,
          },
          action.payload.sessionId,
          {
            ...emptySessionView(),
            agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
            agents: mergeAgents([], action.payload.agents ?? []),
            sessionRuntime: action.payload.sessionRuntime ?? null,
            ...interactionState(new Map(), action.payload.interactions ?? [], action.payload.sessionId, true),
            currentRunEventBaseSequence: 0,
            turnPhase: "running" as TurnPhase,
            turnStartedAt: action.startedAt,
            isBusy: true,
          },
        ),
        currentRunEventBaseSequence: 0,
        status: action.status,
        turnPhase: "running" as TurnPhase,
        turnStartedAt: action.startedAt,
        isBusy: true,
      };
      return storeSessionView(appendOptimisticWaiting(handoffState, action.startedAt), action.payload.sessionId);
    }
    case "sessionModeUpdated":
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
      return storeSessionView({
        ...state,
        sessions: sessionListOrPrevious(action.payload.sessions, state.sessions),
        selectedSessionId: action.payload.sessionId,
        agentTimelineEvents: mergeAgentTimelineEvents([], action.payload.agentEvents ?? []),
        agents: mergeAgents([], action.payload.agents ?? []),
        sessionRuntime: action.payload.sessionRuntime ?? state.sessionRuntime,
        status: action.status,
      }, action.payload.sessionId);
    case "runPromptFailed":
      if (action.sessionId && action.sessionId !== state.selectedSessionId) {
        return state;
      }
      return storeSessionView({
        ...removeOptimisticWaitingTimelinePartViews(state),
        status: action.status,
        turnPhase: "failed",
        turnStartedAt: null,
        isBusy: false,
        ...clearSessionInteractions(state.interactions, state.selectedSessionId),
        currentRunEventBaseSequence: null,
      });
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
      return reduceMcpHealthChanged(state, action.payload);
    case "lspHealthUpdated":
      return reduceLspHealthChanged(state, action.payload);
    case "interactionChanged":
      return reduceInteractionChanged(state, action.payload, action.status);
    case "studioEvent":
      {
        const reduced = reduceStudioEvent(state, action.envelope, action.status);
        return action.envelope.sessionId === reduced.selectedSessionId
          ? storeSessionView(reduced, action.envelope.sessionId)
          : reduced;
      }
    case "planLifecycleLoaded":
      if (action.sessionId !== state.selectedSessionId) {
        return state;
      }
      return storeSessionView({
        ...state,
        planStates: mergePlanStates(state.planStates, action.planStates ?? []),
        eventNextSequence: Math.max(state.eventNextSequence, action.eventNextSequence),
        status: action.status ?? state.status,
      }, action.sessionId);
    case "stopRequested":
      return { ...state, turnPhase: "stopping", status: action.status };
    case "stopFallback":
      return storeSessionView({
        ...state,
        status: action.status,
        turnPhase: "interrupted",
        turnStartedAt: null,
        isBusy: false,
        ...clearSessionInteractions(state.interactions, state.selectedSessionId),
        currentRunEventBaseSequence: null,
      });
    case "planImplementationSubmitted":
      return storeSessionView(appendOptimisticWaiting(
        {
          ...state,
          isBusy: true,
          status: action.status,
          turnPhase: "running",
          turnStartedAt: action.startedAt,
          ...clearSessionInteractions(state.interactions, state.selectedSessionId),
        },
        action.startedAt,
      ));
    case "promptSubmitted":
      const currentRunEventBaseSequence = state.eventNextSequence;
      return storeSessionView({
        ...appendOptimisticPrompt(
          removeOptimisticTimelinePartViews(state),
          action.prompt,
          action.startedAt,
          action.attachments ?? [],
        ),
        prompt: "",
        isBusy: true,
        status: action.status,
        turnPhase: "running",
        turnStartedAt: action.startedAt,
        currentRunEventBaseSequence,
        ...clearSessionInteractions(state.interactions, state.selectedSessionId),
      });
    default:
      return state;
  }
}

function sessionViewFromState(state: StudioState): SessionViewState {
  return {
    messages: new Map(state.messages),
    partsByMessage: clonePartsByMessage(state.partsByMessage),
    partDeltaAccum: new Map(state.partDeltaAccum),
    messageSequences: new Map(state.messageSequences),
    partSequences: new Map(state.partSequences),
    eventNextSequence: state.eventNextSequence,
    agentTimelineEvents: state.agentTimelineEvents,
    agents: state.agents,
    sessionRuntime: state.sessionRuntime,
    interactions: new Map(state.interactions),
    activeInteractionId: state.activeInteractionId,
    planStates: new Map(state.planStates),
    dismissedPlanId: state.dismissedPlanId,
    currentRunEventBaseSequence: state.currentRunEventBaseSequence,
    turnPhase: state.turnPhase,
    turnStartedAt: state.turnStartedAt,
    isBusy: state.isBusy,
  };
}

function emptySessionView(): SessionViewState {
  return {
    ...emptyTimelineState(),
    agentTimelineEvents: [],
    agents: [],
    sessionRuntime: null,
    interactions: new Map(),
    activeInteractionId: null,
    planStates: new Map(),
    dismissedPlanId: null,
    currentRunEventBaseSequence: null,
    turnPhase: "idle",
    turnStartedAt: null,
    isBusy: false,
  };
}

function restoreSessionView<T extends StudioState>(
  state: T,
  sessionId: string,
  fallback: SessionViewState = emptySessionView(),
): T {
  const view = state.sessionViews.get(sessionId) ?? fallback;
  return {
    ...state,
    messages: new Map(view.messages),
    partsByMessage: clonePartsByMessage(view.partsByMessage),
    partDeltaAccum: new Map(view.partDeltaAccum),
    messageSequences: new Map(view.messageSequences),
    partSequences: new Map(view.partSequences),
    eventNextSequence: view.eventNextSequence,
    agentTimelineEvents: view.agentTimelineEvents,
    agents: view.agents,
    sessionRuntime: view.sessionRuntime,
    interactions: new Map(view.interactions),
    activeInteractionId: selectActiveInteractionId(view.interactions, sessionId, view.isBusy),
    planStates: new Map(view.planStates),
    dismissedPlanId: view.dismissedPlanId,
    currentRunEventBaseSequence: view.currentRunEventBaseSequence,
    turnPhase: view.turnPhase,
    turnStartedAt: view.turnStartedAt,
    isBusy: view.isBusy,
  };
}

function storeSessionView<T extends StudioState>(
  state: T,
  sessionId: string | null | undefined = state.selectedSessionId,
  view: SessionViewState = sessionViewFromState(state),
): T {
  if (!sessionId) {
    return state;
  }
  const sessionViews = new Map(state.sessionViews);
  sessionViews.set(sessionId, view);
  return { ...state, sessionViews };
}

function replaceSessionView<T extends StudioState>(
  state: T,
  sessionId: string,
  view: SessionViewState,
): T {
  const sessionViews = new Map(state.sessionViews);
  sessionViews.set(sessionId, view);
  return { ...state, sessionViews };
}

function viewAsState(state: StudioState, sessionId: string, view: SessionViewState): StudioState {
  return {
    ...state,
    selectedSessionId: sessionId,
    messages: new Map(view.messages),
    partsByMessage: clonePartsByMessage(view.partsByMessage),
    partDeltaAccum: new Map(view.partDeltaAccum),
    messageSequences: new Map(view.messageSequences),
    partSequences: new Map(view.partSequences),
    eventNextSequence: view.eventNextSequence,
    agentTimelineEvents: view.agentTimelineEvents,
    agents: view.agents,
    sessionRuntime: view.sessionRuntime,
    interactions: new Map(view.interactions),
    activeInteractionId: view.activeInteractionId,
    planStates: new Map(view.planStates),
    dismissedPlanId: view.dismissedPlanId,
    currentRunEventBaseSequence: view.currentRunEventBaseSequence,
    turnPhase: view.turnPhase,
    turnStartedAt: view.turnStartedAt,
    isBusy: view.isBusy,
  };
}

function stateAsView(state: StudioState): SessionViewState {
  return sessionViewFromState(state);
}

function clonePartsByMessage(
  partsByMessage: Map<string, StudioPart[]>,
): Map<string, StudioPart[]> {
  return new Map(
    [...partsByMessage.entries()].map(([messageId, parts]) => [
      messageId,
      parts.map((part) => ({ ...part })),
    ]),
  );
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

function reduceSessionStateLoaded(
  state: StudioState,
  events: StudioEventEnvelope[],
  status?: string,
): StudioState {
  let next = state;
  for (const envelope of events.slice().sort((left, right) => left.sequence - right.sequence)) {
    next = reduceStudioEvent(next, envelope, status);
  }
  return next;
}

function freshSessionStateEvents(
  events: StudioEventEnvelope[],
  currentNextSequence: number,
): StudioEventEnvelope[] {
  return events.filter((event) => event.sequence >= currentNextSequence);
}

function reduceStudioEvent(
  state: StudioState,
  envelope: StudioEventEnvelope,
  status?: string,
): StudioState {
  const withEventCursor = (next: StudioState): StudioState =>
    envelope.kind.type === "messagePartDelta" || envelope.kind.type === "stale"
      ? next
      : { ...next, eventNextSequence: Math.max(next.eventNextSequence, envelope.sequence + 1) };
  const eventSessionId = envelope.sessionId ?? null;
  const kind = envelope.kind;
  if (kind.type === "sessionListChanged") {
    return withEventCursor({
      ...state,
      sessions: sessionList(kind.sessions),
      status: status ?? state.status,
    });
  }
  if (kind.type === "sessionHandoffChanged") {
    return withEventCursor(reduceSessionHandoffEvent(state, kind.handoff, status));
  }
  if (eventSessionId && eventSessionId !== state.selectedSessionId) {
    const viewState = viewAsState(
      state,
      eventSessionId,
      state.sessionViews.get(eventSessionId) ?? emptySessionView(),
    );
    const reduced = reduceStudioEvent(
      {
        ...viewState,
        selectedSessionId: eventSessionId,
      },
      envelope,
      status,
    );
    return replaceSessionView(state, eventSessionId, stateAsView(reduced));
  }
  const reduced = (() => {
    switch (kind.type) {
    case "turnChanged":
      return reduceTurnChanged(state, kind.turn.status, kind.turn.reason, status);
    case "messageUpdated":
    case "messageRemoved":
    case "messagePartUpdated":
    case "messagePartRemoved":
    case "messagePartDelta":
      return reduceConversationEvent(state, envelope, status);
    case "interactionChanged":
      return reduceInteractionChanged(
        state,
        {
          sessionId: eventSessionId ?? kind.event.interaction.scope.sessionId,
          event: kind.event,
        },
        status,
      );
    case "agentChanged":
      return reduceAgentChanged(state, kind.agent, status);
    case "agentTimelineChanged":
      return reduceAgentTimelineChanged(state, kind.event, status);
    case "sessionRuntimeChanged":
      return reduceSessionRuntimeChanged(state, kind.runtime, status);
    case "skillActivated":
      return reduceSkillActivated(state, kind.activation.name, status);
    case "planLifecycleChanged":
      return {
        ...state,
        planStates: mergePlanStates(state.planStates, [
          {
            planId: kind.event.planId,
            state: kind.event.state,
            turnId: kind.event.turnId ?? null,
            reason: kind.event.reason ?? null,
            updatedAt: kind.event.updatedAt,
          },
        ]),
        status: status ?? state.status,
      };
    case "mcpHealthChanged":
      return reduceMcpHealthChanged(state, kind.health);
    case "lspHealthChanged":
      return reduceLspHealthChanged(state, kind.health);
    case "stale":
      return {
        ...state,
        status: status ?? state.status,
      };
    }
  })();
  return withEventCursor(reduced);
}

function reduceSessionHandoffEvent(
  state: StudioState,
  handoff: Extract<StudioEventEnvelope["kind"], { type: "sessionHandoffChanged" }>["handoff"],
  status?: string,
): StudioState {
  if (state.selectedSessionId !== handoff.originSessionId && state.selectedSessionId !== handoff.targetSessionId) {
    return state;
  }
  const now = Date.now();
  const sessions = state.sessions
    .map((session) =>
      session.id === handoff.originSessionId
        ? { ...session, visibility: "handoffOrigin" as const, updatedAt: handoff.updatedAt }
        : session,
    )
    .filter((session) => session.visibility === "active" || session.id === handoff.targetSessionId);
  const targetExists = sessions.some((session) => session.id === handoff.targetSessionId);
  const nextSessions = targetExists
    ? sessions
    : sessionList([
        {
          id: handoff.targetSessionId,
          projectId: state.selectedProjectId ?? "",
          title: "实施计划",
          mode: "auto",
          updatedAt: handoff.updatedAt,
          visibility: "active",
        },
        ...sessions,
      ]);
  const switched = state.selectedSessionId !== handoff.targetSessionId;
  const nextState: StudioState = {
    ...state,
    sessions: sessionList(nextSessions),
    selectedSessionId: handoff.targetSessionId,
    agentTimelineEvents: switched ? [] : state.agentTimelineEvents,
    agents: switched ? [] : state.agents,
    sessionRuntime: switched ? null : state.sessionRuntime,
    ...(switched ? emptyTimelineState() : {}),
    ...interactionState(
      switched ? new Map() : state.interactions,
      [],
      handoff.targetSessionId,
      handoff.status === "running" || handoff.status === "pending",
    ),
    planStates: switched ? new Map<string, PlanState>() : state.planStates,
    dismissedPlanId: switched ? null : state.dismissedPlanId,
    currentRunEventBaseSequence: switched ? 0 : state.currentRunEventBaseSequence,
    isBusy: handoff.status === "running" || handoff.status === "pending" || state.isBusy,
    turnPhase: handoff.status === "failed" ? "failed" : handoff.status === "cancelled" ? "interrupted" : "running",
    turnStartedAt: state.turnStartedAt ?? now,
    status: status ?? state.status,
  };
  return switched ? appendOptimisticWaiting(nextState, now) : nextState;
}

function reduceTurnChanged(
  state: StudioState,
  status: StudioTurnStatus,
  reason: string | null | undefined,
  statusText?: string,
): StudioState {
  const turnPhase = phaseForStudioTurnStatus(status);
  const terminal = isTerminalStudioTurnStatus(status);
  const base = terminal ? removeOptimisticWaitingTimelinePartViews(state) : state;
  return {
    ...base,
    isBusy: terminal ? false : true,
    turnPhase,
    turnStartedAt: terminal ? null : state.turnStartedAt ?? Date.now(),
    status: reason ? reason : statusText ?? state.status,
    activeInteractionId: selectActiveInteractionId(
      base.interactions,
      base.selectedSessionId,
      terminal ? false : true,
    ),
    currentRunEventBaseSequence: terminal ? null : state.currentRunEventBaseSequence,
  };
}

function reduceAgentChanged(
  state: StudioState,
  agent: AgentDto,
  status?: string,
): StudioState {
  return {
    ...state,
    agents: mergeAgents(state.agents, [agent]),
    status: status ?? state.status,
  };
}

function reduceAgentTimelineChanged(
  state: StudioState,
  event: StudioAgentTimelineEvent,
  status?: string,
): StudioState {
  return {
    ...state,
    agentTimelineEvents: mergeAgentTimelineEvents(
      state.agentTimelineEvents,
      [agentTimelineRecordFromStudioEvent(event)],
    ),
    status: status ?? state.status,
  };
}

function reduceSessionRuntimeChanged(
  state: StudioState,
  runtime: SessionRuntime,
  status?: string,
): StudioState {
  return {
    ...state,
    sessionRuntime: runtime,
    status: status ?? state.status,
  };
}

function reduceSkillActivated(state: StudioState, name: string, status?: string): StudioState {
  if (!state.sessionRuntime || state.sessionRuntime.activeSkills.includes(name)) {
    return { ...state, status: status ?? state.status };
  }
  return {
    ...state,
    sessionRuntime: {
      ...state.sessionRuntime,
      activeSkills: [...state.sessionRuntime.activeSkills, name],
    },
    status: status ?? state.status,
  };
}

function reduceMcpHealthChanged(state: StudioState, payload: McpHealthUpdatedPayload): StudioState {
  return {
    ...state,
    mcpServers: mergeMcpServers(state.mcpServers, payload.mcpServers),
    sessionRuntime: state.sessionRuntime
      ? {
          ...state.sessionRuntime,
          activeMcpServers: payload.activeMcpServers,
        }
      : state.sessionRuntime,
  };
}

function reduceLspHealthChanged(state: StudioState, payload: LspHealthUpdatedPayload): StudioState {
  return {
    ...state,
    lspServers: payload.lspServers,
    sessionRuntime: state.sessionRuntime
      ? {
          ...state.sessionRuntime,
          activeLspServers: payload.activeLspServers,
        }
      : state.sessionRuntime,
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

function appendOptimisticPrompt(
  state: StudioState,
  prompt: string,
  startedAt: number,
  attachments: TimelineAttachment[] = [],
): StudioState {
  const content = prompt.trim();
  if (!content && attachments.length === 0) {
    return state;
  }
  const createdAt = Math.floor(startedAt / 1000);
  const turnId = `optimistic-turn-${startedAt}`;
  const messageId = `optimistic-message-${startedAt}`;
  const userPart: StudioPart = {
    turnId,
    sessionId: state.selectedSessionId ?? "",
    messageId,
    partId: `optimistic-user-${startedAt}`,
    partType: "text",
    order: state.eventNextSequence,
    status: "completed",
    createdAt,
    updatedAt: createdAt,
    completedAt: createdAt,
    textChannel: "user",
    text: content,
    attachments,
  };
  const waitingPart: StudioPart = {
    turnId,
    sessionId: state.selectedSessionId ?? "",
    messageId,
    partId: `optimistic-waiting-${startedAt}`,
    partType: "turn",
    order: state.eventNextSequence + 1,
    status: "running",
    createdAt,
    updatedAt: createdAt,
    textChannel: null,
    text: "waitingForModel",
  };
  const message: StudioMessage = {
    messageId,
    sessionId: state.selectedSessionId ?? "",
    turnId,
    role: "user",
    status: "streaming",
    createdAt,
    updatedAt: createdAt,
  };
  return addOptimisticPart(addOptimisticPart(state, message, userPart), message, waitingPart);
}

function appendOptimisticWaiting(state: StudioState, startedAt: number): StudioState {
  const createdAt = Math.floor(startedAt / 1000);
  const turnId = `optimistic-turn-${startedAt}`;
  const messageId = `optimistic-message-${startedAt}`;
  const waitingPart: StudioPart = {
    turnId,
    sessionId: state.selectedSessionId ?? "",
    messageId,
    partId: `optimistic-waiting-${startedAt}`,
    partType: "turn",
    order: state.eventNextSequence,
    status: "running",
    createdAt,
    updatedAt: createdAt,
    textChannel: null,
    text: "waitingForModel",
  };
  const message: StudioMessage = {
    messageId,
    sessionId: state.selectedSessionId ?? "",
    turnId,
    role: "assistant",
    status: "streaming",
    createdAt,
    updatedAt: createdAt,
  };
  return addOptimisticPart(state, message, waitingPart);
}

function applyOptimisticConversationCleanup(
  state: StudioState,
  kind: StudioEventEnvelope["kind"],
): StudioState {
  let next = state;
  if (shouldClearOptimisticUserPart(kind)) {
    next = removeOptimisticUserTimelinePartViews(next);
  }
  if (shouldClearOptimisticWaitingPart(kind)) {
    next = removeOptimisticWaitingTimelinePartViews(next);
  }
  return next;
}

function shouldClearOptimisticUserPart(kind: StudioEventEnvelope["kind"]): boolean {
  return kind.type === "messagePartUpdated" &&
    kind.part.partType === "text" &&
    kind.part.textChannel === "user";
}

function shouldClearOptimisticWaitingPart(kind: StudioEventEnvelope["kind"]): boolean {
  if (kind.type === "messagePartDelta") {
    return isModelVisibleDeltaField(kind.delta.field);
  }
  if (kind.type !== "messagePartUpdated") {
    return false;
  }
  const part = kind.part;
  if (isModelVisiblePart(part)) {
    return true;
  }
  return part.partType === "turn" && isTerminalTimelineStatus(part.status);
}

function isModelVisiblePart(part: StudioPart): boolean {
  if (part.partType === "text") {
    return part.textChannel === "commentary" || part.textChannel === "final";
  }
  return isModelVisiblePartType(part.partType) || part.partType === "inference" && part.status === "completed";
}

function isModelVisibleDeltaField(field: StudioPartDeltaField): boolean {
  switch (field) {
    case "text":
    case "planContent":
    case "reasoningText":
    case "tool.arguments":
    case "tool.result":
      return true;
  }
}

function isModelVisiblePartType(kind: StudioPart["partType"]): boolean {
  switch (kind) {
    case "text":
    case "reasoning":
    case "tool":
    case "agent":
    case "plan":
      return true;
    case "turn":
    case "inference":
    case "file":
      return false;
  }
}

function isTerminalTimelineStatus(status: TimelinePartView["status"]): boolean {
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

function reduceConversationEvent(
  state: StudioState,
  envelope: StudioEventEnvelope,
  status?: string,
): StudioState {
  const cleaned = applyOptimisticConversationCleanup(state, envelope.kind);
  const next = applyStudioEvent(cleaned, envelope);
  return reduceConversationStatus(next, envelope.kind, status ?? state.status);
}

function reduceConversationStatus(
  state: StudioState,
  kind: StudioEventEnvelope["kind"],
  statusText: string,
): StudioState {
  if (kind.type === "messagePartUpdated") {
    return {
      ...state,
      status: statusText,
      turnPhase: phaseForPart(kind.part, state.turnPhase),
      turnStartedAt: isTerminalTimelineStatus(kind.part.status)
        ? state.turnStartedAt
        : state.turnStartedAt ?? Date.now(),
    };
  }
  if (kind.type === "messagePartDelta") {
    return {
      ...state,
      status: statusText,
      turnPhase: phaseForPartDelta(kind.delta.field, state.turnPhase),
    };
  }
  return { ...state, status: statusText };
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

function agentTimelineRecordFromStudioEvent(event: StudioAgentTimelineEvent): AgentTimelineEvent {
  const index = agentTimelineIndex(event);
  return {
    eventId: event.eventId,
    sessionId: event.sessionId,
    sequence: event.sequence,
    kind: index.kind,
    agentId: index.agentId,
    path: index.path,
    parentPath: index.parentPath,
    payload: null,
    createdAt: event.createdAt,
  };
}

function agentTimelineIndex(event: StudioAgentTimelineEvent): Pick<
  AgentTimelineEvent,
  "kind" | "agentId" | "path" | "parentPath"
> {
  switch (event.kind.type) {
    case "spawnBegin":
      return {
        kind: "spawnBegin",
        agentId: null,
        path: event.kind.senderPath,
        parentPath: null,
      };
    case "spawnEnd":
      return {
        kind: "spawnEnd",
        agentId: event.kind.agentId ?? null,
        path: event.kind.path ?? null,
        parentPath: null,
      };
    case "interactionBegin":
      return {
        kind: "interactionBegin",
        agentId: null,
        path: event.kind.receiverPath,
        parentPath: event.kind.senderPath,
      };
    case "interactionEnd":
      return {
        kind: "interactionEnd",
        agentId: null,
        path: event.kind.receiverPath,
        parentPath: event.kind.senderPath,
      };
    case "waitingBegin":
      return {
        kind: "waitingBegin",
        agentId: null,
        path: event.kind.senderPath,
        parentPath: null,
      };
    case "waitingEnd":
      return {
        kind: "waitingEnd",
        agentId: null,
        path: event.kind.senderPath,
        parentPath: null,
      };
    case "closeBegin":
      return {
        kind: "closeBegin",
        agentId: null,
        path: event.kind.receiverPath,
        parentPath: event.kind.senderPath,
      };
    case "closeEnd":
      return {
        kind: "closeEnd",
        agentId: null,
        path: event.kind.receiverPath,
        parentPath: event.kind.senderPath,
      };
  }
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

function phaseForPart(part: StudioPart, current: TurnPhase): TurnPhase {
  if (current === "stopping") return "stopping";
  if (part.partType === "reasoning") return "thinking";
  if (part.partType === "tool") return "tool";
  if (part.partType === "agent") return "subagent";
  if (part.partType === "plan") return current === "idle" ? "running" : current;
  if (part.status === "failed") return "failed";
  if (part.status === "interrupted") return "interrupted";
  if (part.status === "budgetLimited") return "budgetLimited";
  if (part.partType === "turn" && part.status === "completed") return "completed";
  return "running";
}

function phaseForPartDelta(field: StudioPartDeltaField, current: TurnPhase): TurnPhase {
  if (current === "stopping") return "stopping";
  switch (field) {
    case "reasoningText":
      return "thinking";
    case "tool.arguments":
    case "tool.result":
      return "tool";
    case "planContent":
      return current === "idle" ? "running" : current;
    case "text":
      return "running";
  }
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

function phaseForStudioTurnStatus(status: StudioTurnStatus): TurnPhase {
  switch (status) {
    case "queued":
    case "contextLoading":
    case "waitingForModel":
    case "streaming":
    case "persisting":
      return "running";
    case "waitingForInteraction":
      return "userInput";
    case "runningTool":
      return "tool";
    case "completed":
      return "completed";
    case "failed":
      return "failed";
    case "cancelled":
      return "interrupted";
  }
}

function isTerminalStudioTurnStatus(status: StudioTurnStatus): boolean {
  switch (status) {
    case "completed":
    case "failed":
    case "cancelled":
      return true;
    case "queued":
    case "contextLoading":
    case "waitingForModel":
    case "streaming":
    case "waitingForInteraction":
    case "runningTool":
    case "persisting":
      return false;
  }
}

