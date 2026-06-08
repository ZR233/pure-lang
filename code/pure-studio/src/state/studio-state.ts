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
  UserInputRequest,
  UserInputResolved,
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
export type PlanActionMode = "choice" | "discuss";

export type PlanActionState = {
  planId: string;
  content: string;
  mode: PlanActionMode;
};

type PlanActionEligibility = "completedPlan" | "currentRunPlan";

type PlanActionCandidate = {
  planId: string;
  content: string;
};

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
  pendingUserInput: UserInputRequest | null;
  planAction: PlanActionState | null;
  dismissedPlanId: string | null;
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
  | { type: "userInputRequested"; payload: UserInputRequest; status: string }
  | { type: "userInputResolved"; payload: UserInputResolved; status: string }
  | { type: "setPlanActionMode"; mode: PlanActionMode }
  | { type: "dismissPlanAction" }
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
  | { type: "promptSubmitted"; status: string; startedAt: number; prompt: string };

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
  pendingUserInput: null,
  planAction: null,
  dismissedPlanId: null,
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
        planAction: null,
        dismissedPlanId: null,
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
        pendingUserInput: null,
        planAction: null,
        dismissedPlanId: null,
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
        pendingUserInput: null,
        planAction: null,
        dismissedPlanId: null,
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
    case "runPromptLoaded": {
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
      const currentTimelineItemIds = new Set(
        (action.payload.timelineItems ?? [])
          .map((item) => item.itemId),
      );
      return setPendingPlanAction({
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
        pendingUserInput: null,
      }, currentTimelineItemIds, "currentRunPlan");
    }
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
        pendingUserInput: null,
        planAction: null,
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
    case "userInputRequested":
      if (action.payload.sessionId !== state.selectedSessionId) {
        return state;
      }
      return {
        ...state,
        pendingUserInput: action.payload,
        planAction: null,
        status: action.status,
        turnPhase: "userInput",
      };
    case "userInputResolved":
      if (state.pendingUserInput?.requestId !== action.payload.requestId) {
        return state;
      }
      return {
        ...state,
        pendingUserInput: null,
        status: action.status,
        turnPhase: state.turnPhase === "stopping" ? "stopping" : "tool",
      };
    case "setPlanActionMode":
      return state.planAction
        ? { ...state, planAction: { ...state.planAction, mode: action.mode } }
        : state;
    case "dismissPlanAction":
      return {
        ...state,
        dismissedPlanId: state.planAction?.planId ?? state.dismissedPlanId,
        planAction: null,
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
        pendingUserInput: null,
        planAction: null,
      };
    case "promptSubmitted":
      return {
        ...appendOptimisticPrompt(removeOptimisticTimelineItems(state), action.prompt, action.startedAt),
        prompt: "",
        isBusy: true,
        status: action.status,
        turnPhase: "running",
        turnStartedAt: action.startedAt,
        pendingUserInput: null,
        planAction: null,
      };
    case "agentEvent":
      if (action.sessionId !== state.selectedSessionId) {
        return state;
      }
      const eventBase = shouldClearOptimisticTimeline(action.event)
        ? removeOptimisticTimelineItems(state)
        : state;
      const nextRuntime = mergeLiveSkillIntoRuntime(
        action.sessionRuntime ?? eventBase.sessionRuntime,
        action.event,
      );
      return reduceAgentEvent(
        {
          ...eventBase,
          agentTimelineEvents: action.timelineEvent
            ? mergeAgentTimelineEvents(eventBase.agentTimelineEvents, [action.timelineEvent])
            : eventBase.agentTimelineEvents,
          agents: action.agent ? mergeAgents(eventBase.agents, [action.agent]) : eventBase.agents,
          sessionRuntime: nextRuntime,
        },
        action.event,
        action.statusText,
      );
    default:
      return state;
  }
}

function setPendingPlanAction(
  state: StudioState,
  eligiblePlanIds: Set<string>,
  eligibility: PlanActionEligibility,
): StudioState {
  const plan = latestEligiblePlanCandidate(state, eligiblePlanIds, eligibility);
  if (!plan) {
    return state;
  }
  if (plan.planId === state.dismissedPlanId) {
    return state.planAction ? { ...state, planAction: null } : state;
  }
  const currentPlanAction = state.planAction;
  const samePlanStillPending = currentPlanAction?.planId === plan.planId;
  const mode = samePlanStillPending ? currentPlanAction.mode : "choice";
  const nextPlanAction = {
    planId: plan.planId,
    content: plan.content,
    mode,
  };
  if (
    state.planAction?.planId === nextPlanAction.planId &&
    state.planAction.content === nextPlanAction.content &&
    state.planAction.mode === nextPlanAction.mode
  ) {
    return state;
  }
  return { ...state, planAction: nextPlanAction };
}

function latestEligiblePlanCandidate(
  state: StudioState,
  eligiblePlanIds: Set<string>,
  eligibility: PlanActionEligibility,
): PlanActionCandidate | null {
  for (let index = state.timelineOrder.length - 1; index >= 0; index--) {
    const itemId = state.timelineOrder[index];
    if (!itemId || !eligiblePlanIds.has(itemId)) {
      continue;
    }
    const item = state.timelineItems.get(itemId);
    if (!item?.content.trim()) {
      continue;
    }
    if (item.kind === "plan" && (eligibility === "currentRunPlan" || item.status === "completed")) {
      return { planId: item.itemId, content: item.content };
    }
    if (eligibility === "currentRunPlan" && item.kind === "text" && item.role === "assistant") {
      const proposedPlan = extractProposedPlanContent(item.content);
      if (proposedPlan) {
        return { planId: item.itemId, content: proposedPlan };
      }
    }
  }
  return null;
}

function extractProposedPlanContent(content: string): string | null {
  const match = content.match(/<proposed_plan>\s*([\s\S]*?)\s*<\/proposed_plan>/i);
  const plan = match?.[1]?.trim();
  return plan || null;
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
    timelineNextSequence: nextSequence + 2,
  };
}

function shouldClearOptimisticTimeline(event: AgentEvent | null | undefined): boolean {
  return Boolean(
    event &&
      typeof event === "object" &&
      ("timelineItemStarted" in event ||
        "timelineItemDelta" in event ||
        "timelineItemCompleted" in event ||
        "timelineItemFailed" in event),
  );
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
  if ("userInputRequested" in event) {
    return { ...state, status: statusText, turnPhase: "userInput" };
  }
  if ("userInputAnswered" in event) {
    return {
      ...state,
      status: statusText,
      turnPhase: state.turnPhase === "stopping" ? "stopping" : "tool",
    };
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
    const completed = event.timelineItemCompleted.item;
    return setPendingPlanAction({
      ...nextState,
      status: statusText,
      turnPhase: phaseForTimelineItem(completed, state.turnPhase),
    }, new Set([completed.itemId]), "completedPlan");
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
