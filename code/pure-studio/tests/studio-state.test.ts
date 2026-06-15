import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server.browser";
import { initialStudioState, studioReducer } from "../src/state/studio-state";
import { timelinePartViewsFromConversation } from "../src/state/timeline-state";
import { selectTimelineEntries } from "../src/state/selectors";
import { normalizeRolesForProviders } from "../src/components/RoleSettings";
import { selectedContextWindow } from "../src/components/SessionStatusBar";
import { MarkdownContent } from "../src/components/MarkdownContent";
import {
  applyProviderTemplate,
  cloneProvider,
  createProviderFromTemplate,
  suggestProviderId,
} from "../src/lib/provider-settings";
import { hidesToolResult, isQuietFileTool } from "../src/lib/tool-display";
import { previewTemplates } from "../src/lib/templates";
import type {
  ConfigPayload,
  LspServerRecord,
  McpServerRecord,
  ProviderRecord,
  ProviderUsageRecord,
  PlanState,
  InteractionRequest,
  RoleRecord,
  SessionRuntime,
  StudioEventEnvelope,
  TimelinePartView,
  StudioMessage,
  StudioPart,
  StudioPartDelta,
  ToolCallStatus2,
} from "../src/types";

function assertEqual<T>(actual: T, expected: T) {
  if (actual !== expected) {
    throw new Error(`Expected ${JSON.stringify(actual)} to equal ${JSON.stringify(expected)}`);
  }
}

function assertDeepEqual<T>(actual: T, expected: T) {
  const actualJson = JSON.stringify(actual);
  const expectedJson = JSON.stringify(expected);
  if (actualJson !== expectedJson) {
    throw new Error(`Expected ${actualJson} to equal ${expectedJson}`);
  }
}

function assertIncludes(value: string, expected: string) {
  if (!value.includes(expected)) {
    throw new Error(`Expected ${JSON.stringify(value)} to include ${JSON.stringify(expected)}`);
  }
}

function assertNotIncludes(value: string, expected: string) {
  if (value.includes(expected)) {
    throw new Error(`Expected ${JSON.stringify(value)} not to include ${JSON.stringify(expected)}`);
  }
}

function renderMarkdown(content: string): string {
  return renderToStaticMarkup(createElement(MarkdownContent, { content }));
}

const config: ConfigPayload = {
  toml: "",
  permissionMode: "request-approval",
  instructions: {
    baseOverride: "",
    developer: "",
    user: "",
    projectDocMaxBytes: 65536,
    projectDocFallbackFilenames: [],
  },
  providers: [],
  roles: [],
  templates: [],
  mcpServers: [],
  configExists: false,
};

const runtime: SessionRuntime = {
  sessionId: "session-1",
  usage: {
    model: "model-a",
    latestContextTokens: 0,
    promptTokens: 0,
    completionTokens: 0,
    cachedPromptTokens: 0,
    totalTokens: 0,
    estimatedCosts: [],
    hasUnpricedUsage: false,
    updatedAt: 1,
  },
  activeSkills: [],
  activeMcpServers: [],
  activeLspServers: [],
  updatedAt: 1,
};

function mcpServer(overrides: Partial<McpServerRecord> = {}): McpServerRecord {
  return {
    id: "github",
    enabled: true,
    transport: "streamableHttp",
    command: null,
    args: [],
    env: [],
    cwd: null,
    url: "https://example.com/mcp",
    bearerTokenEnvVar: "GITHUB_MCP_TOKEN",
    headers: [],
    endpoint: "https://example.com/mcp",
    sourceKind: "user",
    sourceLabel: "User",
    sourceDetail: null,
    statusKind: "enabled",
    statusMessage: null,
    mutationPolicy: "userEditable",
    availabilityKind: "checking",
    availabilityMessage: "MCP health check has not completed",
    lastCheckedAt: null,
    toolCount: null,
    ...overrides,
  };
}

function textItem(
  itemId: string,
  sequence: number,
  content: string,
  textChannel: TimelinePartView["textChannel"] = "final",
): TimelinePartView {
  return {
    turnId: itemId.split("-").slice(0, 2).join("-") || "turn",
    itemId,
    startedSequence: sequence,
    kind: "text",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    textChannel,
    content,
    thinkingChunks: [],
  };
}

function planItem(itemId: string, turnId: string, sequence: number, content: string): TimelinePartView {
  return {
    turnId,
    itemId,
    startedSequence: sequence,
    kind: "plan",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    textChannel: null,
    content,
    thinkingChunks: [],
  };
}

function userInputInteraction(interactionId = "call-ask"): InteractionRequest {
  return {
    interactionId,
    kind: "userInput",
    status: "pending",
    scope: {
      sessionId: "session-1",
      turnId: "turn-1",
      itemId: interactionId,
      toolId: interactionId,
    },
    payload: {
      type: "userInput",
      questions: [
        {
          id: "mode",
          header: "Mode",
          question: "Which mode?",
          options: [{ label: "Fast", description: "Use the fast path." }],
        },
      ],
    },
    createdAt: 10,
    updatedAt: 10,
  };
}

function planConfirmationInteraction(
  planId = "turn-1-plan",
  content = "1. Inspect",
): InteractionRequest {
  return {
    interactionId: `plan-confirmation-${planId}`,
    kind: "planConfirmation",
    status: "pending",
    scope: {
      sessionId: "session-1",
      turnId: "turn-1",
      itemId: planId,
    },
    payload: {
      type: "planConfirmation",
      planId,
      content,
    },
    createdAt: 10,
    updatedAt: 10,
  };
}

function thinkingItem(itemId: string, turnId: string, sequence: number, content: string): TimelinePartView {
  return {
    turnId,
    itemId,
    startedSequence: sequence,
    kind: "thinking",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    textChannel: null,
    content: "",
    thinkingChunks: [{ chunkIndex: 0, content }],
  };
}

function toolItem(
  itemId: string,
  turnId: string,
  sequence: number,
  name: string,
  args: Record<string, unknown> | string,
  status: ToolCallStatus2 = "completed",
  result: string | null = null,
): TimelinePartView {
  return {
    turnId,
    itemId,
    startedSequence: sequence,
    kind: "tool",
    status,
    createdAt: sequence,
    updatedAt: sequence,
    textChannel: null,
    content: "",
    thinkingChunks: [],
    tool: {
      toolCallId: `call-${itemId}`,
      name,
      arguments: typeof args === "string" ? args : JSON.stringify(args),
      result,
      exitCode: null,
      timedOut: false,
    },
  };
}

function turnItem(
  itemId: string,
  turnId: string,
  sequence: number,
  status: ToolCallStatus2,
  content = "",
): TimelinePartView {
  return {
    turnId,
    itemId,
    startedSequence: sequence,
    kind: "turn",
    status,
    createdAt: sequence,
    updatedAt: sequence,
    textChannel: null,
    content,
    thinkingChunks: [],
  };
}

function inferenceItem(itemId: string, turnId: string, sequence: number): TimelinePartView {
  return {
    turnId,
    itemId,
    startedSequence: sequence,
    kind: "inference",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    textChannel: null,
    content: "",
    thinkingChunks: [],
    inference: {
      inferenceId: `inference-${itemId}`,
      model: "deepseek-v4-flash",
    },
    usage: {
      promptTokens: 32412,
      completionTokens: 88,
      cachedPromptTokens: 0,
      totalTokens: 32500,
    },
  };
}

function nextSequenceForItems(timelinePartViews: TimelinePartView[]): number {
  return timelinePartViews.reduce((max, item) => Math.max(max, item.startedSequence), -1) + 1;
}

function planState(planId: string, state: PlanState["state"]): PlanState {
  return {
    planId,
    state,
    turnId: null,
    reason: null,
    updatedAt: 20,
  };
}

function selectedState() {
  return studioReducer(initialStudioState("starting"), {
    type: "bootstrapLoaded",
    status: "ready",
    payload: {
      projects: [],
      selectedProjectId: "project-1",
      sessions: [
        {
          id: "session-1",
          projectId: "project-1",
          title: "Session",
          mode: "auto",
          updatedAt: 1,
          visibility: "active",
        },
      ],
      selectedSessionId: "session-1",
      agentEvents: [],
      agents: [],
      interactions: [],
      sessionRuntime: runtime,
      config,
    },
  });
}

type TestState = ReturnType<typeof selectedState>;
type TestTimelinePartDelta = {
  itemId: string;
  field: "content" | "planContent" | "thinkingChunk" | "toolArguments" | "toolResult";
  delta: string;
  chunkIndex?: number;
};
type TestConversationChange =
  | { type: "itemUpdated"; message: StudioMessage; part: StudioPart }
  | { type: "itemDelta"; delta: StudioPartDelta };

function studioEvent(
  kind: StudioEventEnvelope["kind"],
  overrides: Partial<StudioEventEnvelope> = {},
): StudioEventEnvelope {
  return {
    eventId: `event-${Math.random()}`,
    projectId: "project-1",
    sessionId: "session-1",
    turnId: "turn-1",
    sequence: 1,
    createdAt: 1,
    kind,
    ...overrides,
  };
}

function messageForItem(item: TimelinePartView, role: StudioMessage["role"] = "assistant"): StudioMessage {
  const messageId = role === "user" ? `${item.turnId}:user` : `${item.turnId}:assistant`;
  return {
    messageId,
    sessionId: "session-1",
    turnId: item.turnId,
    role,
    status: item.status === "failed"
      ? "failed"
      : item.status === "streaming" || item.status === "started" || item.status === "running"
        ? "streaming"
        : "completed",
    createdAt: item.createdAt,
    updatedAt: item.updatedAt,
    completedAt: item.status === "completed" ? item.updatedAt : null,
    error: item.status === "failed" ? item.content : null,
    metadata: {},
  };
}

function partForItem(item: TimelinePartView): StudioPart {
  return {
    partId: item.itemId,
    messageId: messageForItem(item).messageId,
    sessionId: "session-1",
    turnId: item.turnId,
    partType: item.kind === "thinking" ? "reasoning" : item.kind,
    order: item.startedSequence,
    status: item.status,
    createdAt: item.createdAt,
    updatedAt: item.updatedAt,
    completedAt: item.status === "completed" ? item.updatedAt : null,
    error: item.status === "failed" ? item.content : null,
    textChannel: item.textChannel,
    text: item.kind === "thinking"
      ? item.thinkingChunks.map((chunk) => chunk.content).join("")
      : item.content,
    attachments: item.attachments ?? [],
    tool: item.tool,
    agent: item.agent,
    inference: item.inference,
    plan: item.kind === "plan" ? { content: item.content } : null,
    file: null,
    usage: item.usage ?? null,
    synthetic: false,
    ignored: false,
  };
}

function eventsForItems(items: TimelinePartView[], sessionId = "session-1"): StudioEventEnvelope[] {
  return items.flatMap((item) => {
    const message = { ...messageForItem(item), sessionId };
    const part = { ...partForItem(item), sessionId, messageId: message.messageId };
    return [
      studioEvent(
        { type: "messageUpdated", message },
        { eventId: `event-${item.startedSequence}-message-${message.messageId}`, sessionId, sequence: item.startedSequence, createdAt: item.createdAt },
      ),
      studioEvent(
        { type: "messagePartUpdated", part },
        { eventId: `event-${item.startedSequence}-part-${part.partId}`, sessionId, sequence: item.startedSequence, createdAt: item.updatedAt },
      ),
    ];
  });
}

function loadSessionItems(
  state: TestState,
  items: TimelinePartView[],
  nextSequence = nextSequenceForItems(items),
  planStates?: PlanState[],
  interactions?: InteractionRequest[],
) {
  const messages = items.map((item) => ({
    message: messageForItem(item),
    sequence: item.startedSequence,
  }));
  const parts = items.map((item) => ({
    part: partForItem(item),
    sequence: item.startedSequence,
  }));
  return studioReducer(state, {
    type: "sessionStateLoaded",
    sessionId: "session-1",
    messages,
    parts,
    events: [],
    nextSequence,
    planStates,
    interactions,
  });
}

function timelinePartViews(state: TestState): Map<string, TimelinePartView> {
  return new Map(timelinePartViewsFromConversation(state).map((item) => [item.itemId, item]));
}

function timelinePartView(state: TestState, itemId: string): TimelinePartView | undefined {
  return timelinePartViews(state).get(itemId);
}

function timelineOrder(state: TestState): string[] {
  return timelinePartViewsFromConversation(state).map((item) => item.itemId);
}

function studioPartEvent(
  part: StudioPart,
  sequence: number,
  overrides: Partial<StudioEventEnvelope> = {},
): StudioEventEnvelope {
  return studioEvent(
    {
      type: "messagePartUpdated",
      part,
    },
    {
      eventId: `event-${sequence}`,
      sequence,
      createdAt: sequence,
      ...overrides,
    },
  );
}

function itemUpdated(item: TimelinePartView): TestConversationChange {
  return { type: "itemUpdated", message: messageForItem(item), part: partForItem(item) };
}

function itemDelta(delta: TestTimelinePartDelta): TestConversationChange {
  const field: StudioPartDelta["field"] =
    delta.field === "content"
      ? "text"
      : delta.field === "thinkingChunk"
        ? "reasoningText"
        : delta.field === "toolArguments"
          ? "tool.arguments"
          : delta.field === "toolResult"
            ? "tool.result"
            : "planContent";
  const studioDelta = partDelta(delta.itemId, field, delta.delta);
  return {
    type: "itemDelta",
    delta: {
      ...studioDelta,
      chunkIndex: delta.chunkIndex ?? studioDelta.chunkIndex ?? null,
    },
  };
}

function studioTimelineEvent(
  change: TestConversationChange,
  sequence: number,
  overrides: Partial<StudioEventEnvelope> = {},
): StudioEventEnvelope {
  return change.type === "itemUpdated"
    ? studioPartEvent(change.part, sequence, overrides)
    : studioPartDeltaEvent(change.delta, sequence, overrides);
}

function studioItemEvents(
  item: TimelinePartView,
  sequence: number,
  overrides: Partial<StudioEventEnvelope> = {},
): StudioEventEnvelope[] {
  const message = messageForItem(item);
  const part = partForItem(item);
  return [
    studioEvent(
      { type: "messageUpdated", message },
      { eventId: `event-${sequence}-message`, sequence, createdAt: sequence, ...overrides },
    ),
    studioPartEvent(part, sequence, overrides),
  ];
}

function reduceStudioEvents(state: TestState, events: StudioEventEnvelope[], status = "running") {
  return events.reduce(
    (next, envelope) => studioReducer(next, { type: "studioEvent", envelope, status }),
    state,
  );
}

function applyStudioConversationChange(
  state: TestState,
  change: TestConversationChange,
  sequence: number,
  status = "running",
) {
  if (change.type === "itemUpdated") {
    const withMessage = studioReducer(state, {
      type: "studioEvent",
      envelope: studioEvent(
        { type: "messageUpdated", message: change.message },
        { eventId: `event-${sequence}-message`, sequence, createdAt: sequence },
      ),
      status,
    });
    return studioReducer(withMessage, {
      type: "studioEvent",
      envelope: studioPartEvent(change.part, sequence),
      status,
    });
  }
  return studioReducer(state, {
    type: "studioEvent",
    envelope: studioTimelineEvent(change, sequence),
    status,
  });
}

function studioPartDeltaEvent(
  delta: StudioPartDelta,
  sequence: number,
  overrides: Partial<StudioEventEnvelope> = {},
): StudioEventEnvelope {
  return studioEvent(
    {
      type: "messagePartDelta",
      delta,
    },
    {
      eventId: `event-${sequence}`,
      sequence,
      createdAt: sequence,
      ...overrides,
    },
  );
}

function applyStudioPartEvent(
  state: ReturnType<typeof selectedState>,
  part: StudioPart,
  sequence: number,
  status = "running",
) {
  return studioReducer(state, {
    type: "studioEvent",
    envelope: studioPartEvent(part, sequence),
    status,
  });
}

function applyStudioItemUpdate(
  state: ReturnType<typeof selectedState>,
  item: TimelinePartView,
  sequence: number,
  status = "running",
) {
  return applyStudioPartEvent(state, partForItem(item), sequence, status);
}

function partDelta(itemId: string, field: StudioPartDelta["field"], delta: string): StudioPartDelta {
  const turnId = itemId.split("-").slice(0, 2).join("-") || "turn";
  return {
    sessionId: "session-1",
    messageId: `${turnId}:assistant`,
    partId: itemId,
    field,
    delta,
    chunkIndex: field === "reasoningText" ? 0 : null,
  };
}

function applyStudioPartDelta(
  state: ReturnType<typeof selectedState>,
  delta: StudioPartDelta,
  sequence: number,
  status = "running",
) {
  return studioReducer(state, {
    type: "studioEvent",
    envelope: studioPartDeltaEvent(delta, sequence),
    status,
  });
}

function completeTurn(
  state: TestState,
  sequence: number,
  status = "done",
  sessionId = "session-1",
  turnId = "turn-1",
) {
  return studioReducer(state, {
    type: "studioEvent",
    envelope: studioEvent(
      {
        type: "turnChanged",
        turn: {
          turnId,
          sessionId,
          status: "completed",
          reason: null,
          updatedAt: sequence,
        },
      },
      { sessionId, turnId, sequence, createdAt: sequence },
    ),
    status,
  });
}

function entriesForTimeline(items: TimelinePartView[]) {
  const loaded = loadSessionItems(selectedState(), items);
  return selectTimelineEntries(loaded);
}

function sessionStateProjectionSnapshotRestoresTimelineWithoutEvents() {
  const item = textItem("turn-1-text", 10, "hello from projection");
  const message = messageForItem(item);
  const part = partForItem(item);
  const loaded = studioReducer(selectedState(), {
    type: "sessionStateLoaded",
    sessionId: "session-1",
    messages: [{ message, sequence: 40 }],
    parts: [{ part, sequence: 41 }],
    events: [],
    nextSequence: 42,
  });

  assertDeepEqual(timelineOrder(loaded), ["turn-1-text"]);
  assertEqual(timelinePartView(loaded, "turn-1-text")?.content, "hello from projection");
  assertEqual(loaded.eventNextSequence, 42);
}

function sessionStateEventsRestoreDurableStatusSnapshots() {
  const updatedRuntime = {
    ...runtime,
    activeSkills: ["openai-docs"],
    updatedAt: 20,
  };
  const loaded = studioReducer(selectedState(), {
    type: "sessionStateLoaded",
    sessionId: "session-1",
    messages: [],
    parts: [],
    events: [
      studioEvent(
        {
          type: "planLifecycleChanged",
          event: {
            planId: "turn-1-plan",
            state: "dismissed",
            turnId: "turn-1",
            reason: "done",
            updatedAt: 20,
          },
        },
        { sequence: 40 },
      ),
      studioEvent(
        {
          type: "sessionRuntimeChanged",
          runtime: updatedRuntime,
        },
        { sequence: 41 },
      ),
    ],
    nextSequence: 42,
  });

  assertEqual(loaded.planStates.get("turn-1-plan")?.state, "dismissed");
  assertDeepEqual(loaded.sessionRuntime?.activeSkills, ["openai-docs"]);
  assertEqual(loaded.eventNextSequence, 42);
}

function staleTimelineLoadKeepsNewTurnItems() {
  const oldItem = textItem("turn-1-text", 1, "old");
  const newItem = textItem("turn-2-text", 10, "new");
  const withNewTurn = applyStudioConversationChange(selectedState(), itemUpdated(newItem), 10, "done");

  const afterStaleLoad = loadSessionItems(withNewTurn, [oldItem], 2);

  assertDeepEqual(timelineOrder(afterStaleLoad), ["turn-1-text", "turn-2-text"]);
  assertEqual(timelinePartView(afterStaleLoad, "turn-1-text")?.content, "old");
  assertEqual(timelinePartView(afterStaleLoad, "turn-2-text")?.content, "new");
  assertEqual(afterStaleLoad.eventNextSequence, 11);
}

function freshTimelineLoadMayReplaceSnapshot() {
  const firstItem = textItem("turn-1-text", 1, "first");
  const replacement = textItem("turn-1-text", 2, "replacement");
  const loaded = loadSessionItems(selectedState(), [firstItem], 2);

  const refreshed = loadSessionItems(loaded, [replacement], 3);

  assertDeepEqual(timelineOrder(refreshed), ["turn-1-text"]);
  assertEqual(timelinePartView(refreshed, "turn-1-text")?.content, "replacement");
  assertEqual(refreshed.eventNextSequence, 3);
}

function staleTimelineLoadDoesNotOverwriteLiveDelta() {
  const oldItem = textItem("turn-1-text", 1, "old");
  const started = {
    ...textItem("turn-2-text", 10, ""),
    status: "streaming" as const,
  };
  const delta: TestTimelinePartDelta = {
    itemId: "turn-2-text",
    field: "content",
    delta: "new",
  };
  const completed = {
    ...textItem("turn-2-text", 10, "new"),
    updatedAt: 12,
  };
  const liveStarted = applyStudioConversationChange(selectedState(), itemUpdated(started), 10);
  const liveDelta = applyStudioConversationChange(liveStarted, itemDelta(delta), 11);
  const liveCompleted = applyStudioConversationChange(liveDelta, itemUpdated(completed), 12, "done");

  const afterStaleLoad = loadSessionItems(liveCompleted, [oldItem], 2);

  assertDeepEqual(timelineOrder(afterStaleLoad), ["turn-1-text", "turn-2-text"]);
  assertEqual(timelinePartView(afterStaleLoad, "turn-1-text")?.content, "old");
  assertEqual(timelinePartView(afterStaleLoad, "turn-2-text")?.content, "new");
  assertEqual(afterStaleLoad.eventNextSequence, 13);
}

function staleSessionStateDoesNotReplayOldStatusEvents() {
  const live = {
    ...completeTurn(selectedState(), 20),
    sessionRuntime: {
      ...runtime,
      activeSkills: ["new-skill"],
      updatedAt: 20,
    },
    status: "done",
  };
  const old = studioReducer(live, {
    type: "sessionStateLoaded",
    sessionId: "session-1",
    sessionRuntime: runtime,
    messages: [],
    parts: [],
    events: [
      studioEvent(
        {
          type: "turnChanged",
          turn: {
            turnId: "turn-1",
            sessionId: "session-1",
            status: "streaming",
            reason: null,
            updatedAt: 1,
          },
        },
        { sequence: 10 },
      ),
    ],
    nextSequence: 11,
  });

  assertEqual(old.turnPhase, "completed");
  assertDeepEqual(old.sessionRuntime?.activeSkills, ["new-skill"]);
  assertEqual(old.status, "done");
  assertEqual(old.eventNextSequence, 21);
}

function toolArgumentDeltaRequiresSnapshot() {
  const delta: TestTimelinePartDelta = {
    itemId: "turn-1-call-1",
    field: "toolArguments",
    delta: "{\"path\":\"a.ts\"",
  };
  const started = toolItem("turn-1-call-1", "turn-1", 9, "read_file", "");
  started.status = "streaming";
  const orphan = applyStudioConversationChange(selectedState(), itemDelta(delta), 10);
  const withStart = applyStudioConversationChange(orphan, itemUpdated(started), 9);
  const withDelta = applyStudioConversationChange(withStart, itemDelta(delta), 10);
  const completed = toolItem("turn-1-call-1", "turn-1", 9, "read_file", "{\"path\":\"a.ts\"");
  completed.updatedAt = 11;
  const withCompleted = applyStudioConversationChange(withDelta, itemUpdated(completed), 11, "done");

  assertEqual(timelinePartViews(orphan).has("turn-1-call-1"), false);
  const liveGroup = selectTimelineEntries(withDelta)[0];
  if (liveGroup?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${liveGroup?.kind}`);
  }
  assertEqual(liveGroup.items[0]?.tool?.arguments, "{\"path\":\"a.ts\"");
  const tool = timelinePartView(withCompleted, "turn-1-call-1")?.tool;
  assertEqual(tool?.name, "read_file");
  assertEqual(tool?.arguments, "{\"path\":\"a.ts\"");
}

function toolResultDeltaRequiresSnapshot() {
  const delta: TestTimelinePartDelta = {
    itemId: "turn-1-call-1",
    field: "toolResult",
    delta: "partial result",
  };
  const started = toolItem("turn-1-call-1", "turn-1", 9, "read_file", { path: "a.ts" }, "running");
  const orphan = applyStudioConversationChange(selectedState(), itemDelta(delta), 10);
  const withStart = applyStudioConversationChange(orphan, itemUpdated(started), 9);
  const withDelta = applyStudioConversationChange(withStart, itemDelta(delta), 10);
  const completed = toolItem(
    "turn-1-call-1",
    "turn-1",
    9,
    "read_file",
    { path: "a.ts" },
    "completed",
    "partial result",
  );
  completed.updatedAt = 11;
  const withCompleted = applyStudioConversationChange(withDelta, itemUpdated(completed), 11, "done");

  assertEqual(timelinePartViews(orphan).has("turn-1-call-1"), false);
  const liveGroup = selectTimelineEntries(withDelta)[0];
  if (liveGroup?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${liveGroup?.kind}`);
  }
  assertEqual(liveGroup.items[0]?.tool?.result, "partial result");
  const tool = timelinePartView(withCompleted, "turn-1-call-1")?.tool;
  assertEqual(tool?.name, "read_file");
  assertEqual(tool?.arguments, "{\"path\":\"a.ts\"}");
  assertEqual(tool?.result, "partial result");
}

function textDeltaRequiresSnapshotForCommentaryChannel() {
  const delta: TestTimelinePartDelta = {
    itemId: "turn-1-commentary",
    field: "content",
    delta: "正在检查 CI 配置。",
  };
  const started = {
    ...textItem("turn-1-commentary", 10, "", "commentary"),
    status: "streaming" as const,
  };
  const orphan = applyStudioConversationChange(selectedState(), itemDelta(delta), 9);
  const withStart = applyStudioConversationChange(orphan, itemUpdated(started), 10);
  const withDelta = applyStudioConversationChange(withStart, itemDelta(delta), 11);
  const item = timelinePartView(withDelta, "turn-1-commentary");
  const entries = selectTimelineEntries(withDelta);

  assertEqual(timelinePartViews(orphan).has("turn-1-commentary"), false);
  assertEqual(item?.textChannel, "commentary");
  assertEqual(item?.content, "");
  assertDeepEqual(entries.map((entry) => entry.kind), ["commentary"]);
  const commentary = entries[0];
  if (commentary?.kind !== "commentary") {
    throw new Error("Expected commentary entry");
  }
  assertEqual(commentary.content, "正在检查 CI 配置。");
}

function turnCompletedWithNoPartsDoesNotDeleteLiveContent() {
  const liveItem = textItem("turn-2-text", 10, "live");
  const liveState = applyStudioConversationChange(selectedState(), itemUpdated(liveItem), 10, "done");

  const completed = completeTurn(liveState, 20);

  assertDeepEqual(timelineOrder(completed), ["turn-2-text"]);
  assertEqual(timelinePartView(completed, "turn-2-text")?.content, "live");
  assertEqual(completed.eventNextSequence, 21);
}

function staleEventDoesNotAdvanceCursor() {
  const before = selectedState();
  const state = studioReducer(before, {
    type: "studioEvent",
    envelope: studioEvent(
      { type: "stale", laggedEvents: 5 },
      { sequence: 50 },
    ),
    status: "refreshing",
  });

  assertEqual(state.eventNextSequence, before.eventNextSequence);
  assertEqual(state.status, "refreshing");
}

function runPromptFailedKeepsFailedStatusText() {
  const state = studioReducer(selectedState(), {
    type: "runPromptFailed",
    sessionId: "session-1",
    status: "运行失败：provider error",
  });

  assertEqual(state.turnPhase, "failed");
  assertEqual(state.status, "运行失败：provider error");
}

function userInputRequestStoresPendingComposerState() {
  const interaction = userInputInteraction();
  const state = studioReducer(selectedState(), {
    type: "interactionChanged",
    payload: {
      sessionId: "session-1",
      event: { interaction },
    },
    status: "waiting",
  });

  assertEqual(state.turnPhase, "userInput");
  assertEqual(state.activeInteractionId, interaction.interactionId);
  assertDeepEqual(state.interactions.get(interaction.interactionId), interaction);
}

function userInputResolvedClearsPendingComposerState() {
  const pendingInteraction = userInputInteraction();
  const pending = studioReducer(selectedState(), {
    type: "interactionChanged",
    payload: {
      sessionId: "session-1",
      event: { interaction: pendingInteraction },
    },
    status: "waiting",
  });
  const resolvedInteraction: InteractionRequest = {
    ...pendingInteraction,
    status: "resolved",
    resolvedAt: 11,
    updatedAt: 11,
    resolution: { type: "userInput", answers: {} },
  };

  const resolved = studioReducer(pending, {
    type: "interactionChanged",
    payload: {
      sessionId: "session-1",
      event: { interaction: resolvedInteraction },
    },
    status: "answered",
  });

  assertEqual(resolved.activeInteractionId, null);
  assertEqual(resolved.turnPhase, "tool");
}

function completedSnapshotKeepsFirstItemSequence() {
  const started = {
    ...textItem("turn-2-text", 10, ""),
    status: "streaming" as const,
  };
  const completed = {
    ...textItem("turn-2-text", 10, "final"),
    updatedAt: 12,
  };
  const liveStarted = applyStudioConversationChange(selectedState(), itemUpdated(started), 10);

  const liveCompleted = applyStudioConversationChange(liveStarted, itemUpdated(completed), 12, "done");

  assertDeepEqual(timelineOrder(liveCompleted), ["turn-2-text"]);
  assertEqual(timelinePartView(liveCompleted, "turn-2-text")?.startedSequence, 10);
  assertEqual(timelinePartView(liveCompleted, "turn-2-text")?.content, "final");
  assertEqual(liveCompleted.eventNextSequence, 13);
}

function terminalSnapshotClearsLiveDeltaOverlay() {
  const started = {
    ...textItem("turn-2-text", 10, ""),
    status: "streaming" as const,
  };
  const liveDelta = applyStudioConversationChange(
    applyStudioConversationChange(selectedState(), itemUpdated(started), 10),
    itemDelta({
      itemId: "turn-2-text",
      field: "content",
      delta: "partial",
    }),
    11,
  );
  const liveMessage = selectTimelineEntries(liveDelta).find(
    (entry) => entry.kind === "message" && entry.content === "partial",
  );
  const completed = applyStudioConversationChange(
    liveDelta,
    itemUpdated({
      ...textItem("turn-2-text", 10, "final"),
      updatedAt: 12,
    }),
    12,
    "done",
  );
  const finalEntries = selectTimelineEntries(completed);

  assertEqual(liveMessage?.kind === "message" ? liveMessage.content : "", "partial");
  assertEqual(completed.partDeltaAccum.has("turn-2-text"), false);
  assertEqual(timelinePartView(completed, "turn-2-text")?.content, "final");
  assertDeepEqual(finalEntries.map((entry) => entry.kind), ["message"]);
  assertEqual(finalEntries[0]?.kind === "message" ? finalEntries[0].content : "", "final");
}

function realtimeAndHistoricalTimelineEventsConverge() {
  const started = {
    ...textItem("turn-2-text", 10, ""),
    status: "streaming" as const,
  };
  const delta: TestTimelinePartDelta = {
    itemId: "turn-2-text",
    field: "content",
    delta: "hel",
  };
  const completed = {
    ...textItem("turn-2-text", 10, "hello"),
    updatedAt: 12,
  };
  const live = applyStudioConversationChange(
    applyStudioConversationChange(
      applyStudioConversationChange(selectedState(), itemUpdated(started), 10),
      itemDelta(delta),
      11,
    ),
    itemUpdated(completed),
    12,
    "done",
  );
  const historical = loadSessionItems(selectedState(), [completed], 13);

  assertDeepEqual(timelineOrder(historical), timelineOrder(live));
  assertDeepEqual(
    timelinePartView(historical, "turn-2-text"),
    timelinePartView(live, "turn-2-text"),
  );
  assertEqual(historical.eventNextSequence, live.eventNextSequence);
}

function consecutiveToolsCollapseIntoToolGroup() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read-a", "turn-1", 1, "read_file", { path: "a.ts" }),
    toolItem("turn-1-read-many", "turn-1", 2, "search_files", { paths: ["b.ts", "c.ts"] }),
    toolItem("turn-1-edit", "turn-1", 3, "write_file", { path: "d.ts" }),
  ]);

  assertEqual(entries.length, 1);
  const entry = entries[0];
  if (entry?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${entry?.kind}`);
  }
  assertEqual(entry.turnId, "turn-1");
  assertEqual(entry.items.length, 3);
  assertDeepEqual(entry.summaryParts, [
    { kind: "readFiles", count: 3 },
    { kind: "editFiles", count: 1 },
  ]);
}

function thinkingDoesNotBreakToolGroup() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read-a", "turn-1", 1, "read_file", { path: "a.ts" }),
    thinkingItem("turn-1-thinking", "turn-1", 2, "checking the next file"),
    toolItem("turn-1-read-b", "turn-1", 3, "read_file", { path: "b.ts" }),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["toolGroup", "thought"]);
  const group = entries[0];
  if (group?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${group?.kind}`);
  }
  assertEqual(group.items.length, 2);
  assertDeepEqual(group.summaryParts, [{ kind: "readFiles", count: 2 }]);
  const thought = entries[1];
  if (thought?.kind !== "thought") {
    throw new Error(`Expected thought entry, got ${thought?.kind}`);
  }
  assertEqual(thought.content, "checking the next file");
}

function inferenceDoesNotBreakToolGroup() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read-a", "turn-1", 1, "read_file", { path: "a.ts" }),
    inferenceItem("turn-1-inference", "turn-1", 2),
    toolItem("turn-1-read-b", "turn-1", 3, "read_file", { path: "b.ts" }),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["toolGroup"]);
  const group = entries[0];
  if (group?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${group?.kind}`);
  }
  assertEqual(group.items.length, 2);
  assertDeepEqual(group.summaryParts, [{ kind: "readFiles", count: 2 }]);
}

function repeatedThinkingItemsCollapseIntoOneThought() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read-a", "turn-1", 1, "read_file", { path: "a.ts" }),
    thinkingItem("turn-1-thinking-a", "turn-1", 2, "checking a"),
    toolItem("turn-1-read-b", "turn-1", 3, "read_file", { path: "b.ts" }),
    inferenceItem("turn-1-inference", "turn-1", 4),
    thinkingItem("turn-1-thinking-b", "turn-1", 5, "checking b"),
    toolItem("turn-1-read-c", "turn-1", 6, "read_file", { path: "c.ts" }),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["toolGroup", "thought"]);
  const group = entries[0];
  const thought = entries[1];
  if (group?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${group?.kind}`);
  }
  if (thought?.kind !== "thought") {
    throw new Error(`Expected thought entry, got ${thought?.kind}`);
  }
  assertEqual(group.items.length, 3);
  assertDeepEqual(group.summaryParts, [{ kind: "readFiles", count: 3 }]);
  assertEqual(thought.content, "checking a\n\nchecking b");
}

function thinkingFromDifferentTurnsDoesNotMerge() {
  const entries = entriesForTimeline([
    thinkingItem("turn-1-thinking", "turn-1", 1, "first turn"),
    thinkingItem("turn-2-thinking", "turn-2", 2, "second turn"),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["thought", "thought"]);
  const first = entries[0];
  const second = entries[1];
  if (first?.kind !== "thought" || second?.kind !== "thought") {
    throw new Error("Expected both entries to be thought");
  }
  assertEqual(first.content, "first turn");
  assertEqual(second.content, "second turn");
}

function assistantTextBreaksToolGroup() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read", "turn-1", 1, "read_file", { path: "a.ts" }),
    textItem("turn-1-text", 2, "agent text"),
    toolItem("turn-1-edit", "turn-1", 3, "write_file", { path: "b.ts" }),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["toolGroup", "message", "toolGroup"]);
  const message = entries[1];
  if (message?.kind !== "message") {
    throw new Error(`Expected message entry, got ${message?.kind}`);
  }
  assertEqual(message.content, "agent text");
}

function planEntryBreaksToolGroupAndRendersAsPlan() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read", "turn-1", 1, "read_file", { path: "a.ts" }),
    planItem("turn-1-plan", "turn-1", 2, "1. Read\n2. Implement"),
    toolItem("turn-1-read-b", "turn-1", 3, "read_file", { path: "b.ts" }),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["toolGroup", "plan", "toolGroup"]);
  const plan = entries[1];
  if (plan?.kind !== "plan") {
    throw new Error(`Expected plan entry, got ${plan?.kind}`);
  }
  assertEqual(plan.content, "1. Read\n2. Implement");
}

function livePlanDeltaCreatesPlanEntry() {
  const started = {
    ...planItem("turn-1-plan", "turn-1", 10, ""),
    status: "streaming" as const,
  };
  const delta: TestTimelinePartDelta = {
    itemId: "turn-1-plan",
    field: "planContent",
    delta: "1. Inspect\n",
  };
  const liveStarted = applyStudioConversationChange(selectedState(), itemUpdated(started), 10);
  const liveDelta = applyStudioConversationChange(liveStarted, itemDelta(delta), 11);
  const entries = selectTimelineEntries(liveDelta);

  assertDeepEqual(entries.map((entry) => entry.kind), ["plan"]);
  const plan = entries[0];
  if (plan?.kind !== "plan") {
    throw new Error(`Expected plan entry, got ${plan?.kind}`);
  }
  assertEqual(plan.content, "1. Inspect\n");
  assertEqual(timelinePartView(liveDelta, "turn-1-plan")?.content, "");
  assertEqual(liveDelta.activeInteractionId, null);
}

function interactionEventStoresPlanConfirmation() {
  const interaction = planConfirmationInteraction();
  const state = studioReducer(selectedState(), {
    type: "interactionChanged",
    payload: { sessionId: "session-1", event: { interaction } },
    status: "done",
  });

  assertEqual(state.activeInteractionId, interaction.interactionId);
  assertDeepEqual(state.interactions.get(interaction.interactionId), interaction);
}

function messagePartUpdatedWithoutInteractionDoesNotInferPlanConfirmation() {
  const state = loadSessionItems(
    selectedState(),
    [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    11,
  );

  assertEqual(state.activeInteractionId, null);
}

function planLifecycleLoadedKeepsInteractionStateSeparate() {
  const interaction = planConfirmationInteraction();
  const withPlan = studioReducer(selectedState(), {
    type: "interactionChanged",
    payload: { sessionId: "session-1", event: { interaction } },
    status: "ready",
  });
  const dismissed = studioReducer(withPlan, {
    type: "planLifecycleLoaded",
    sessionId: "session-1",
    planStates: [planState("turn-1-plan", "dismissed")],
    eventNextSequence: 12,
    status: "ready",
  });

  assertEqual(dismissed.activeInteractionId, interaction.interactionId);
  assertEqual(dismissed.planStates.get("turn-1-plan")?.state, "dismissed");
  assertEqual(dismissed.eventNextSequence, 12);
}

function sessionStateLoadedPlanStateAnnotatesPlanWithoutOpeningComposer() {
  const state = loadSessionItems(
    selectedState(),
    [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    13,
    [planState("turn-1-plan", "dismissed")],
  );
  const entries = selectTimelineEntries(state);
  const plan = entries.find((entry) => entry.kind === "plan");

  assertEqual(state.activeInteractionId, null);
  assertEqual(plan?.kind, "plan");
  if (plan?.kind !== "plan") {
    throw new Error("expected plan entry");
  }
  assertEqual(plan.planState?.state, "dismissed");
}

function sessionStateLoadedNewPlanStatesAnnotatePlan() {
  for (const state of ["pendingConfirmation", "continuedPlanning", "cancelled"] as const) {
    const reduced = loadSessionItems(
      selectedState(),
      [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
      13,
      [planState("turn-1-plan", state)],
    );
    const plan = selectTimelineEntries(reduced).find((entry) => entry.kind === "plan");

    if (plan?.kind !== "plan") {
      throw new Error("expected plan entry");
    }
    assertEqual(plan.planState?.state, state);
  }
}

function historicalTimelineLoadDoesNotCreatePlanAction() {
  const state = loadSessionItems(
    selectedState(),
    [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    11,
  );

  assertEqual(state.activeInteractionId, null);
}

function laterRunWithoutPlanDoesNotReopenHistoricalPlan() {
  const withHistory = loadSessionItems(
    selectedState(),
    [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    11,
  );
  const submitted = studioReducer(withHistory, {
    type: "promptSubmitted",
    status: "running",
    startedAt: 100,
    prompt: "continue",
  });
  const completed = completeTurn(
    applyStudioConversationChange(submitted, itemUpdated(textItem("turn-2-text", 20, "continue")), 20),
    21,
  );

  assertEqual(completed.activeInteractionId, null);
}

function interactionEventKeepsLiveInteractionByCurrentRun() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 100,
    prompt: "make a plan",
  });
  const turnCompleted = completeTurn(submitted, 12);
  const interaction = planConfirmationInteraction("turn-1-plan", "1. Inspect");
  const completed = studioReducer(turnCompleted, {
    type: "interactionChanged",
    payload: { sessionId: "session-1", event: { interaction } },
    status: "done",
  });

  assertEqual(completed.activeInteractionId, interaction.interactionId);
  assertDeepEqual(completed.interactions.get(interaction.interactionId), interaction);
}

function freshContextRunSwitchesToReturnedSession() {
  const planningSession = selectedState();
  const submitted = studioReducer(planningSession, {
    type: "planImplementationSubmitted",
    status: "running",
    startedAt: 2222,
  });
  const resolving = studioReducer(submitted, {
    type: "interactionChanged",
    payload: {
      sessionId: "session-1",
      event: {
        interaction: {
          ...planConfirmationInteraction(),
          status: "resolved",
          resolvedAt: 2,
        },
      },
    },
    status: "ready",
  });
  const handoff = studioReducer(resolving, {
    type: "sessionHandoffStarted",
    status: "running",
    startedAt: 3333,
    payload: {
      sessionId: "session-2",
      sessions: [
        {
          id: "session-2",
          projectId: "project-1",
          title: "Implementation",
          mode: "auto",
          updatedAt: 3,
          visibility: "active",
        },
      ],
      agentEvents: [],
      agents: [],
      sessionRuntime: { ...runtime, sessionId: "session-2" },
      interactions: [],
    },
  });
  const withImplementation = reduceStudioEvents(
    handoff,
    studioItemEvents(
      { ...textItem("turn-2-text", 20, "implemented"), turnId: "turn-2" },
      20,
      { sessionId: "session-2", turnId: "turn-2" },
    ),
    "running",
  );
  const completed = completeTurn(withImplementation, 21, "done", "session-2", "turn-2");

  assertEqual(completed.selectedSessionId, "session-2");
  assertEqual(timelinePartView(completed, "turn-2-text")?.content, "implemented");
  assertEqual(completed.isBusy, false);
  assertDeepEqual(completed.sessions.map((session) => session.id), ["session-2"]);
}

function planImplementationSubmittedShowsWaitingWithoutSwitchingSession() {
  const submitted = studioReducer(selectedState(), {
    type: "planImplementationSubmitted",
    status: "running",
    startedAt: 2222,
  });
  const entries = selectTimelineEntries(submitted);

  assertEqual(submitted.selectedSessionId, "session-1");
  assertEqual(submitted.isBusy, true);
  assertEqual(submitted.turnPhase, "running");
  assertEqual(entries[0]?.kind, "status");
}

function sessionHandoffStartedSwitchesBeforeLiveEvents() {
  const planningSession = selectedState();
  const liveItem = {
    ...textItem("turn-2-text", 20, ""),
    status: "streaming" as const,
  };
  const handoff = studioReducer(planningSession, {
    type: "sessionHandoffStarted",
    status: "running",
    startedAt: 3333,
    payload: {
      sessionId: "session-2",
      sessions: [
        {
          id: "session-2",
          projectId: "project-1",
          title: "Implementation",
          mode: "auto",
          updatedAt: 3,
          visibility: "active",
        },
      ],
      agentEvents: [],
      agents: [],
      sessionRuntime: { ...runtime, sessionId: "session-2" },
      interactions: [],
    },
  });
  const started = reduceStudioEvents(
    handoff,
    studioItemEvents(liveItem, 20, { sessionId: "session-2" }),
    "running",
  );

  assertEqual(handoff.selectedSessionId, "session-2");
  assertEqual(handoff.isBusy, true);
  assertEqual(handoff.turnPhase, "running");
  assertEqual(timelinePartViews(started).has("turn-2-text"), true);
}

function studioTurnEventShowsWaitingState() {
  const state = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioEvent({
      type: "turnChanged",
      turn: {
        turnId: "turn-1",
        sessionId: "session-1",
        status: "waitingForModel",
        reason: null,
        updatedAt: 10,
      },
    }),
    status: "waiting",
  });

  assertEqual(state.isBusy, true);
  assertEqual(state.turnPhase, "running");
  assertEqual(state.status, "waiting");
}

function studioTimelineEventClearsWaitingAndStreamsContent() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    prompt: "Build",
    status: "running",
    startedAt: 1234,
  });
  const liveItem = {
    ...textItem("turn-1-text", 20, ""),
    status: "streaming" as const,
  };
  const started = applyStudioConversationChange(submitted, itemUpdated(liveItem), 20);
  const delta = studioReducer(started, {
    type: "studioEvent",
    envelope: studioTimelineEvent(
      itemDelta({
        itemId: "turn-1-text",
        field: "content",
        delta: "live",
      }),
      21,
    ),
    status: "running",
  });

  assertEqual(timelinePartViews(delta).has("optimistic-waiting-1234"), false);
  assertEqual(timelinePartView(delta, "turn-1-text")?.content, "");
  const entries = selectTimelineEntries(delta);
  const message = entries.find((entry) => entry.kind === "message" && entry.content === "live");
  assertEqual(message?.kind === "message" ? message.content : "", "live");
  assertEqual(delta.eventNextSequence, 21);
}

function studioTimelineEventUsesEnvelopeSequenceForCursor() {
  const item = {
    ...textItem("turn-1-text", 0, ""),
    status: "streaming" as const,
  };
  const started = applyStudioConversationChange(selectedState(), itemUpdated(item), 30);
  const delta = studioReducer(started, {
    type: "studioEvent",
    envelope: studioTimelineEvent(
      itemDelta({
        itemId: "turn-1-text",
        field: "content",
        delta: "canonical",
      }),
      31,
      { eventId: "event-31", sequence: 31 },
    ),
    status: "running",
  });

  assertEqual(timelinePartView(delta, "turn-1-text")?.content, "");
  const entries = selectTimelineEntries(delta);
  const message = entries.find((entry) => entry.kind === "message");
  assertEqual(message?.kind === "message" ? message.content : "", "canonical");
  assertEqual(timelinePartView(delta, "turn-1-text")?.startedSequence, 0);
  assertEqual(delta.eventNextSequence, 31);
  assertEqual(delta.partDeltaAccum.has("turn-1-text"), true);
}

function studioHandoffEventSwitchesToTargetSession() {
  const state = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioEvent(
      {
        type: "sessionHandoffChanged",
        handoff: {
          originSessionId: "session-1",
          targetSessionId: "session-2",
          kind: "planImplementation",
          status: "running",
          planId: "turn-1-plan",
          updatedAt: 20,
        },
      },
      { sessionId: "session-2" },
    ),
    status: "running",
  });

  assertEqual(state.selectedSessionId, "session-2");
  assertEqual(state.isBusy, true);
  assertEqual(state.turnPhase, "running");
  assertDeepEqual(state.sessions.map((session) => session.id), ["session-2"]);
}

function studioSessionRuntimeEventUpdatesActiveSkills() {
  const updatedRuntime = {
    ...runtime,
    activeSkills: ["skill-creator"],
    updatedAt: 20,
  };
  const state = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioEvent({
      type: "sessionRuntimeChanged",
      runtime: updatedRuntime,
    }),
    status: "running",
  });

  assertDeepEqual(state.sessionRuntime?.activeSkills, ["skill-creator"]);
}

function liveEventsForTargetAreIgnoredBeforeHandoffStarted() {
  const liveItem = {
    ...textItem("turn-2-text", 20, ""),
    status: "streaming" as const,
  };
  const ignored = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioTimelineEvent(itemUpdated(liveItem), 20, { sessionId: "session-2" }),
    status: "running",
  });

  assertEqual(timelinePartViews(ignored).has("turn-2-text"), false);
}

function sessionSelectionDedupesReturnedSessions() {
  const completed = studioReducer(selectedState(), {
    type: "sessionSelectionLoaded",
    payload: {
      sessionId: "session-1",
      sessions: [
        {
          id: "session-1",
          projectId: "project-1",
          title: "Session",
          mode: "auto",
          updatedAt: 2,
          visibility: "active",
        },
        {
          id: "session-1",
          projectId: "project-1",
          title: "Duplicate",
          mode: "auto",
          updatedAt: 1,
          visibility: "active",
        },
      ],
      agentEvents: [],
      agents: [],
      sessionRuntime: runtime,
      interactions: [],
    },
    status: "done",
  });

  assertDeepEqual(completed.sessions.map((session) => session.title), ["Session"]);
}

function sessionModeUpdateNoLongerSwitchesForFreshContextRun() {
  const planningSession = selectedState();
  const switched = studioReducer(planningSession, {
    type: "sessionModeUpdated",
    status: "mode updated",
    payload: {
      sessionId: "session-2",
      sessions: [
        ...planningSession.sessions,
        {
          id: "session-2",
          projectId: "project-1",
          title: "Implementation",
          mode: "auto",
          updatedAt: 3,
          visibility: "active",
        },
      ],
      agentEvents: [],
      agents: [],
      sessionRuntime: { ...runtime, sessionId: "session-2" },
      interactions: [],
    },
  });

  assertEqual(switched.selectedSessionId, "session-1");
  assertEqual(switched.isBusy, false);
  assertEqual(switched.turnPhase, "idle");
}

function promptSubmittedCreatesOptimisticTimelineFeedback() {
  const before = selectedState();
  const state = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const entries = selectTimelineEntries(state);

  assertDeepEqual(entries.map((entry) => entry.kind), ["message", "status"]);
  const message = entries[0];
  const status = entries[1];
  if (message?.kind !== "message" || status?.kind !== "status") {
    throw new Error("Expected optimistic message and status entries");
  }
  assertEqual(message.role, "user");
  assertEqual(message.content, "Build the thing");
  assertEqual(status.status, "running");
  assertEqual(status.content, "waitingForModel");
  assertDeepEqual(timelineOrder(state), ["optimistic-user-1234", "optimistic-waiting-1234"]);
  assertEqual(state.eventNextSequence, before.eventNextSequence);
  assertEqual(state.prompt, "");
  assertEqual(state.isBusy, true);
}

function modelTimelineEventClearsWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const realItem = textItem("turn-1-text", 10, "Working");
  const updated = applyStudioConversationChange(submitted, itemUpdated(realItem), 10);

  assertDeepEqual(timelineOrder(updated), ["optimistic-user-1234", "turn-1-text"]);
  assertEqual(timelinePartViews(updated).has("optimistic-user-1234"), true);
  assertEqual(timelinePartViews(updated).has("optimistic-waiting-1234"), false);
}

function userTimelineEventKeepsWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const realUser = textItem(
    "turn-1-user",
    submitted.eventNextSequence,
    "Build the thing",
    "user",
  );
  const updated = applyStudioConversationChange(submitted, itemUpdated(realUser), realUser.startedSequence);
  const entries = selectTimelineEntries(updated);

  assertDeepEqual(entries.map((entry) => entry.kind), ["message", "status"]);
  const message = entries[0];
  const status = entries[1];
  if (message?.kind !== "message" || status?.kind !== "status") {
    throw new Error("Expected real user message followed by waiting status");
  }
  assertEqual(message.content, "Build the thing");
  assertEqual(status.content, "waitingForModel");
  assertEqual(timelinePartViews(updated).has("optimistic-user-1234"), false);
  assertEqual(timelinePartViews(updated).has("optimistic-waiting-1234"), true);
  assertDeepEqual(timelineOrder(updated), ["turn-1-user", "optimistic-waiting-1234"]);
}

function inferenceStartKeepsWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const inference: TimelinePartView = {
    turnId: "turn-1",
    itemId: "turn-1-inf-0",
    startedSequence: 2,
    kind: "inference",
    status: "running",
    createdAt: 2,
    updatedAt: 2,
    textChannel: null,
    content: "",
    thinkingChunks: [],
    inference: {
      inferenceId: "turn-1-inf-0",
      model: "model-a",
    },
  };
  const updated = applyStudioConversationChange(submitted, itemUpdated(inference), 2);

  assertEqual(timelinePartViews(updated).has("optimistic-waiting-1234"), true);
}

function assistantSnapshotClearsOptimisticWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const loaded = applyStudioConversationChange(
    submitted,
    itemUpdated(textItem("turn-1-text", 10, "Done")),
    10,
    "done",
  );

  assertDeepEqual(timelineOrder(loaded), ["optimistic-user-1234", "turn-1-text"]);
  assertEqual(timelinePartViews(loaded).has("optimistic-user-1234"), true);
  assertEqual(timelinePartViews(loaded).has("optimistic-waiting-1234"), false);
}

function runPromptFailedClearsWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const failed = studioReducer(submitted, {
    type: "runPromptFailed",
    sessionId: "session-1",
    status: "failed",
  });
  const entries = selectTimelineEntries(failed);

  assertDeepEqual(entries.map((entry) => entry.kind), ["message"]);
  assertEqual(timelinePartViews(failed).has("optimistic-user-1234"), true);
  assertEqual(timelinePartViews(failed).has("optimistic-waiting-1234"), false);
}

function thinkingEntriesExposeStreamingStatusAndDuration() {
  const item = thinkingItem("turn-1-thinking", "turn-1", 10, "Inspecting context");
  item.status = "streaming";
  item.createdAt = 100;
  item.updatedAt = 100;
  const entries = entriesForTimeline([item]);
  const thought = entries[0];

  if (thought?.kind !== "thought") {
    throw new Error(`Expected thought entry, got ${thought?.kind}`);
  }
  assertEqual(thought.status, "streaming");
  assertEqual(thought.durationSeconds, 0);
}

function completedThinkingEntryUsesDuration() {
  const item = thinkingItem("turn-1-thinking", "turn-1", 10, "Inspecting context");
  item.createdAt = 10;
  item.updatedAt = 135;
  const entries = entriesForTimeline([item]);
  const thought = entries[0];

  if (thought?.kind !== "thought") {
    throw new Error(`Expected thought entry, got ${thought?.kind}`);
  }
  assertEqual(thought.status, "completed");
  assertEqual(thought.durationSeconds, 125);
}

function liveCompletedPlanDoesNotCreatePlanInteraction() {
  const item = planItem("turn-1-plan", "turn-1", 10, "1. Inspect");
  const state = applyStudioConversationChange(selectedState(), itemUpdated(item), 10);

  assertEqual(state.activeInteractionId, null);
}

function sessionSelectionClearsInteractionState() {
  const interaction = planConfirmationInteraction();
  const withPlan = studioReducer(selectedState(), {
    type: "interactionChanged",
    payload: { sessionId: "session-1", event: { interaction } },
    status: "done",
  });
  const switched = studioReducer(withPlan, {
    type: "sessionSelectionLoaded",
    status: "loaded",
    payload: {
      sessionId: "session-2",
      sessions: [
        {
          id: "session-2",
          projectId: "project-1",
          title: "Session 2",
          mode: "auto",
          updatedAt: 3,
          visibility: "active",
        },
      ],
      agentEvents: [],
      agents: [],
      interactions: [],
      sessionRuntime: runtime,
    },
  });

  assertEqual(switched.activeInteractionId, null);
  assertEqual(switched.interactions.size, 0);
}

function selectingCurrentSessionKeepsTimelineState() {
  const loaded = loadSessionItems(selectedState(), [textItem("turn-1-text", 10, "hello")], 11);
  const withPlan = studioReducer(loaded, {
    type: "studioEvent",
    envelope: studioTimelineEvent(itemUpdated(planItem("turn-1-plan", "turn-1", 12, "1. Inspect")), 12),
    status: "done",
  });
  const withInteraction = studioReducer(withPlan, {
    type: "interactionChanged",
    payload: { sessionId: "session-1", event: { interaction: planConfirmationInteraction() } },
    status: "done",
  });
  const selectedAgain = studioReducer(withInteraction, {
    type: "sessionSelectionLoaded",
    status: "loaded",
    payload: {
      sessionId: "session-1",
      sessions: withInteraction.sessions,
      agentEvents: [],
      agents: [],
      interactions: [planConfirmationInteraction()],
      sessionRuntime: runtime,
    },
  });

  assertDeepEqual(timelineOrder(selectedAgain), timelineOrder(withInteraction));
  assertEqual(timelinePartView(selectedAgain, "turn-1-plan")?.content, "1. Inspect");
  assertEqual(selectedAgain.activeInteractionId, withInteraction.activeInteractionId);
  assertEqual(selectedAgain.eventNextSequence, withInteraction.eventNextSequence);
}

function toolsFromDifferentTurnsDoNotMerge() {
  const entries = entriesForTimeline([
    toolItem("turn-1-read", "turn-1", 1, "read_file", { path: "a.ts" }),
    toolItem("turn-2-read", "turn-2", 2, "read_file", { path: "b.ts" }),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["toolGroup", "toolGroup"]);
  const first = entries[0];
  const second = entries[1];
  if (first?.kind !== "toolGroup" || second?.kind !== "toolGroup") {
    throw new Error("Expected both entries to be toolGroup");
  }
  assertEqual(first.turnId, "turn-1");
  assertEqual(second.turnId, "turn-2");
}

function toolGroupStatusUsesPriority() {
  const failedEntries = entriesForTimeline([
    toolItem("turn-1-read", "turn-1", 1, "read_file", { path: "a.ts" }, "completed"),
    toolItem("turn-1-run", "turn-1", 2, "bash", { command: "npm test" }, "running"),
    toolItem("turn-1-approval", "turn-1", 3, "write_file", { path: "b.ts" }, "awaitingApproval"),
    toolItem("turn-1-failed", "turn-1", 4, "write_file", { path: "c.ts" }, "failed"),
  ]);
  const failedGroup = failedEntries[0];
  if (failedGroup?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${failedGroup?.kind}`);
  }
  assertEqual(failedGroup.status, "failed");

  const awaitingEntries = entriesForTimeline([
    toolItem("turn-2-run", "turn-2", 1, "bash", { command: "npm test" }, "running"),
    toolItem("turn-2-approval", "turn-2", 2, "write_file", { path: "b.ts" }, "awaitingApproval"),
  ]);
  const awaitingGroup = awaitingEntries[0];
  if (awaitingGroup?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${awaitingGroup?.kind}`);
  }
  assertEqual(awaitingGroup.status, "awaitingApproval");
}

function sessionModeUpdateKeepsTimelineAndUpdatesSessions() {
  const liveItem = planItem("turn-1-plan", "turn-1", 10, "1. Inspect");
  const liveState = applyStudioConversationChange(selectedState(), itemUpdated(liveItem), 10);

  const updated = studioReducer(liveState, {
    type: "sessionModeUpdated",
    status: "mode updated",
    payload: {
      sessionId: "session-1",
      sessions: [
        {
          id: "session-1",
          projectId: "project-1",
          title: "Session",
          mode: "plan",
          updatedAt: 3,
          visibility: "active",
        },
      ],
      agentEvents: [],
      agents: [],
      sessionRuntime: runtime,
    },
  });

  assertEqual(updated.sessions[0]?.mode, "plan");
  assertDeepEqual(timelineOrder(updated), ["turn-1-plan"]);
  assertEqual(timelinePartView(updated, "turn-1-plan")?.content, "1. Inspect");
}

function lspServer(overrides: Partial<LspServerRecord> = {}): LspServerRecord {
  return {
    id: "rust-analyzer",
    displayName: "rust-analyzer",
    extensions: [".rs"],
    languageIds: ["rust"],
    availabilityKind: "checking",
    availabilityMessage: "LSP health check has not completed",
    lastCheckedAt: null,
    diagnosticCount: 0,
    activityKind: "idle",
    activityTitle: null,
    activityMessage: null,
    activityPercentage: null,
    lastError: null,
    lastErrorAt: null,
    ...overrides,
  };
}

function projectSelectionLoadedAfterSessionDeleteClearsSelectedSession() {
  const liveItem = textItem("turn-1-text", 10, "hello");
  const liveState = loadSessionItems(selectedState(), [liveItem], 11);
  const updated = studioReducer(liveState, {
    type: "projectSelectionLoaded",
    status: "deleted",
    payload: {
      projectId: "project-1",
      projects: [],
      sessions: [],
      selectedSessionId: null,
      agentEvents: [],
      agents: [],
      sessionRuntime: null,
    },
  });

  assertEqual(updated.selectedSessionId, null);
  assertDeepEqual(updated.sessions, []);
  assertDeepEqual(timelineOrder(updated), []);
  assertEqual(updated.sessionRuntime, null);
  assertEqual(updated.status, "deleted");
}

function projectSelectionLoadedCanClearSelectedProject() {
  const liveState = loadSessionItems(
    selectedState(),
    [textItem("turn-1-text", 10, "hello")],
    11,
  );
  const updated = studioReducer(liveState, {
    type: "projectSelectionLoaded",
    status: "archived",
    payload: {
      selectedProjectId: null,
      projects: [],
      sessions: [],
      selectedSessionId: null,
      agentEvents: [],
      agents: [],
      sessionRuntime: null,
    },
  });

  assertEqual(updated.selectedProjectId, null);
  assertEqual(updated.selectedSessionId, null);
  assertDeepEqual(updated.projects, []);
  assertDeepEqual(updated.sessions, []);
  assertDeepEqual(timelineOrder(updated), []);
  assertEqual(updated.sessionRuntime, null);
}

function configLoadedUpdatesPermissionMode() {
  const state = studioReducer(selectedState(), {
    type: "configLoaded",
    payload: {
      ...config,
      permissionMode: "auto-review",
      mcpServers: [mcpServer()],
    },
    status: "saved",
  });

  assertEqual(state.permissionMode, "auto-review");
  assertDeepEqual(state.mcpServers.map((server) => server.id), ["github"]);
  assertEqual(state.status, "saved");
}

function providerUsagesLoadedDoesNotReplaceConfigState() {
  const deepseekProvider = createProviderFromTemplate(template("deepseek"), "deepseek");
  const roles: RoleRecord[] = [
    {
      key: "planner",
      displayName: "Planner",
      provider: "deepseek",
      model: deepseekProvider.defaultModel,
      effort: "medium",
    },
  ];
  const state = {
    ...selectedState(),
    providers: [deepseekProvider],
    roles,
    providerTemplates: previewTemplates,
    selectedProviderId: "deepseek",
    configToml: "before",
  };
  const usage: ProviderUsageRecord = {
    providerId: "deepseek",
    updatedAt: 10,
    status: "ready",
    usageKind: "deepseekBalance",
    message: null,
    balance: {
      isAvailable: true,
      balances: [
        {
          currency: "CNY",
          totalBalance: "8.00",
          grantedBalance: "3.00",
          toppedUpBalance: "5.00",
        },
      ],
    },
    codingPlan: null,
  };

  const updated = studioReducer(state, {
    type: "providerUsagesLoaded",
    usages: [usage],
  });

  assertDeepEqual(updated.providers, state.providers);
  assertDeepEqual(updated.roles, roles);
  assertDeepEqual(updated.providerTemplates, previewTemplates);
  assertEqual(updated.selectedProviderId, "deepseek");
  assertEqual(updated.configToml, "before");
  assertEqual(updated.providerUsages[0]?.providerId, "deepseek");

  const refreshed = studioReducer(updated, {
    type: "providerUsagesLoaded",
    usages: [{ ...usage, updatedAt: 20 }],
  });

  assertEqual(refreshed.providerUsages.length, 1);
  assertEqual(refreshed.providerUsages[0]?.updatedAt, 20);
}

function mcpHealthUpdatedRefreshesMcpServersAndRuntime() {
  const state = {
    ...selectedState(),
    mcpServers: [mcpServer({ availabilityKind: "checking" })],
    sessionRuntime: {
      ...runtime,
      activeMcpServers: [],
    },
  };

  const updated = studioReducer(state, {
    type: "mcpHealthUpdated",
    payload: {
      mcpServers: [
        mcpServer({
          availabilityKind: "available",
          availabilityMessage: "Available with 3 tools",
          lastCheckedAt: 123,
          toolCount: 3,
        }),
      ],
      activeMcpServers: ["github"],
    },
  });

  assertEqual(updated.mcpServers[0]?.availabilityKind, "available");
  assertEqual(updated.mcpServers[0]?.toolCount, 3);
  assertDeepEqual(updated.sessionRuntime?.activeMcpServers, ["github"]);
}

function lspHealthUpdatedRefreshesLspServersAndRuntime() {
  const state = {
    ...selectedState(),
    lspServers: [lspServer({ availabilityKind: "checking" })],
    sessionRuntime: {
      ...runtime,
      activeLspServers: [],
    },
  };

  const updated = studioReducer(state, {
    type: "lspHealthUpdated",
    payload: {
      lspServers: [
        lspServer({
          availabilityKind: "available",
          availabilityMessage: "Available",
          lastCheckedAt: 123,
          diagnosticCount: 2,
          activityKind: "indexing",
          activityTitle: "Roots Scanned",
          activityMessage: "166/408",
          activityPercentage: 40,
          lastError: "previous error",
          lastErrorAt: 456,
        }),
      ],
      activeLspServers: ["rust-analyzer"],
    },
  });

  assertEqual(updated.lspServers[0]?.availabilityKind, "available");
  assertEqual(updated.lspServers[0]?.diagnosticCount, 2);
  assertEqual(updated.lspServers[0]?.activityKind, "indexing");
  assertEqual(updated.lspServers[0]?.activityMessage, "166/408");
  assertEqual(updated.lspServers[0]?.lastError, "previous error");
  assertDeepEqual(updated.sessionRuntime?.activeLspServers, ["rust-analyzer"]);
}

function applyPatchCountsFilesFromResultSummary() {
  const entries = entriesForTimeline([
    toolItem(
      "turn-1-patch",
      "turn-1",
      1,
      "apply_patch",
      { patch: "*** Begin Patch\n*** End Patch" },
      "completed",
      "M src/a.ts\nA src/b.ts\nD src/c.ts",
    ),
  ]);

  const group = entries[0];
  if (group?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${group?.kind}`);
  }
  assertDeepEqual(group.summaryParts, [{ kind: "editFiles", count: 3 }]);
}

function skillActivationEventUsesSessionRuntimePayload() {
  const state = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioEvent({
      type: "sessionRuntimeChanged",
      runtime: {
        ...runtime,
        activeSkills: ["openai-docs", "skill-creator"],
        updatedAt: 2,
      },
    }),
    status: "running",
  });

  assertDeepEqual(state.sessionRuntime?.activeSkills, ["openai-docs", "skill-creator"]);
}

function skillViewToolResultJsonNoLongerUpdatesActiveSkills() {
  const skillTool = toolItem(
    "turn-1-skill",
    "turn-1",
    10,
    "skill_view",
    { name: "skill-creator" },
    "completed",
    JSON.stringify({
      success: true,
      skill: { name: "skill-creator" },
      content: "body",
    }),
  );
  const state = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioTimelineEvent(itemUpdated(skillTool), 10),
    status: "running",
  });

  assertDeepEqual(state.sessionRuntime?.activeSkills, []);
}

function skillActivationFromOtherSessionDoesNotUpdateActiveSkills() {
  const state = studioReducer(selectedState(), {
    type: "studioEvent",
    envelope: studioEvent(
      {
        type: "sessionRuntimeChanged",
        runtime: {
          ...runtime,
          sessionId: "session-2",
          activeSkills: ["skill-creator"],
          updatedAt: 2,
        },
      },
      { sessionId: "session-2" },
    ),
    status: "running",
  });

  assertDeepEqual(state.sessionRuntime?.activeSkills, []);
}

function unknownToolsUseFallbackAndKeepDetails() {
  const entries = entriesForTimeline([
    toolItem("turn-1-custom-a", "turn-1", 1, "custom_tool", { value: 1 }),
    toolItem("turn-1-custom-b", "turn-1", 2, "another_tool", { value: 2 }),
  ]);

  const group = entries[0];
  if (group?.kind !== "toolGroup") {
    throw new Error(`Expected toolGroup entry, got ${group?.kind}`);
  }
  assertDeepEqual(group.summaryParts, [{ kind: "useTools", count: 2 }]);
  assertDeepEqual(
    group.items.map((item) => item.tool?.name),
    ["custom_tool", "another_tool"],
  );
}

function quietFileToolFailureKeepsResultVisible() {
  assertEqual(isQuietFileTool("read_file"), true);
  assertEqual(hidesToolResult("read_file", "completed"), true);
  assertEqual(hidesToolResult("read_file", "failed"), false);
  assertEqual(hidesToolResult("search_files", "interrupted"), false);
  assertEqual(hidesToolResult("bash", "completed"), false);
}

function inferenceAndNormalTurnTraceAreHidden() {
  const entries = entriesForTimeline([
    inferenceItem("turn-1-inference", "turn-1", 1),
    turnItem("turn-1-started", "turn-1", 2, "running"),
    turnItem("turn-1-completed", "turn-1", 3, "completed"),
    textItem("turn-1-text", 4, "agent text"),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["message"]);
  const message = entries[0];
  if (message?.kind !== "message") {
    throw new Error(`Expected message entry, got ${message?.kind}`);
  }
  assertEqual(message.content, "agent text");
}

function abnormalTurnTraceIsKeptWithContent() {
  const entries = entriesForTimeline([
    turnItem("turn-1-failed", "turn-1", 1, "failed", "provider error"),
    turnItem("turn-2-interrupted", "turn-2", 2, "interrupted", "stopped by user"),
    turnItem("turn-3-budget", "turn-3", 3, "budgetLimited", "budget limit reached"),
  ]);

  assertDeepEqual(entries.map((entry) => entry.kind), ["trace", "trace", "trace"]);
  assertDeepEqual(
    entries.map((entry) => (entry.kind === "trace" ? entry.item.content : "")),
    ["provider error", "stopped by user", "budget limit reached"],
  );
}

function liveFailedTimelinePartViewKeepsErrorMessage() {
  const item = turnItem(
    "turn-1-turn",
    "turn-1",
    10,
    "failed",
    "LLM provider error: missing API key",
  );
  const state = applyStudioConversationChange(
    selectedState(),
    itemUpdated(item),
    10,
    "error",
  );

  const entries = selectTimelineEntries(state);

  assertEqual(timelinePartView(state, "turn-1-turn")?.content, "LLM provider error: missing API key");
  assertDeepEqual(entries.map((entry) => entry.kind), ["trace"]);
  assertEqual(
    entries[0]?.kind === "trace" ? entries[0].item.content : "",
    "LLM provider error: missing API key",
  );
}

function template(kind: "deepseek" | "openai" | "zhipu" | "zhipu-coding-plan") {
  const item = previewTemplates.find((candidate) => candidate.id === kind);
  if (!item) {
    throw new Error(`Missing ${kind} template`);
  }
  return item;
}

function providerDraftUsesSingleAddEntryAndUniqueKey() {
  const deepseek = template("deepseek");
  const existing = [
    createProviderFromTemplate(deepseek, "deepseek"),
    createProviderFromTemplate(deepseek, "deepseek-2"),
  ];
  const nextId = suggestProviderId(existing, deepseek.id);
  const draft = createProviderFromTemplate(deepseek, nextId);

  assertEqual(nextId, "deepseek-3");
  assertEqual(draft.id, "deepseek-3");
  assertEqual(draft.templateKind, "deepseek");
  assertEqual(draft.defaultModel, deepseek.defaultModel);
}

function providerDraftTemplateSwitchUpdatesTemplateFields() {
  const deepseek = template("deepseek");
  const openai = template("openai");
  const draft = createProviderFromTemplate(deepseek, "deepseek");
  const switched = applyProviderTemplate(draft, openai, { id: "openai", name: openai.name });

  assertEqual(switched.id, "openai");
  assertEqual(switched.name, "OpenAI");
  assertEqual(switched.templateKind, "openai");
  assertEqual(switched.baseUrl, openai.baseUrl);
  assertEqual(switched.providerKind, openai.providerKind);
  assertEqual(switched.defaultModel, openai.defaultModel);
  assertDeepEqual(
    switched.defaultModels.map((model) => model.slug),
    openai.defaultModels.map((model) => model.slug),
  );
}

function openAiTemplateUsesCodexModelMetadata() {
  const openai = template("openai");
  const gpt55 = openai.defaultModels.find((model) => model.slug === "gpt-5.5");
  const gpt54 = openai.defaultModels.find((model) => model.slug === "gpt-5.4");

  assertDeepEqual(
    openai.defaultModels.map((model) => model.slug),
    ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini"],
  );
  assertEqual(gpt55?.contextWindow, 272_000);
  assertEqual(gpt55?.maxContextWindow, 272_000);
  assertEqual(gpt55?.maxOutputTokens, null);
  assertEqual(gpt55?.currency ?? null, null);
  assertDeepEqual(gpt55?.reasoningEfforts, ["medium", "low", "high", "xhigh"]);
  assertEqual(gpt54?.maxContextWindow, 1_000_000);
}

function providerDraftTemplateSwitchSupportsZhipu() {
  const deepseek = template("deepseek");
  const zhipu = template("zhipu");
  const draft = createProviderFromTemplate(deepseek, "deepseek");
  const switched = applyProviderTemplate(draft, zhipu, {
    id: "zhipu",
    name: zhipu.name,
  });

  assertEqual(switched.id, "zhipu");
  assertEqual(switched.name, "Zhipu");
  assertEqual(switched.templateKind, "zhipu");
  assertEqual(switched.baseUrl, "https://open.bigmodel.cn/api/paas/v4");
  assertEqual(switched.providerKind, "zhipu");
  assertEqual(switched.defaultModel, "glm-5.2");
  assertDeepEqual(
    switched.defaultModels.map((model) => model.slug),
    ["glm-5.2", "glm-5", "glm-5-turbo", "glm-4.7", "glm-4.7-flashx", "glm-4.7-flash"],
  );
}

function providerDraftTemplateSwitchSupportsZhipuCodingPlan() {
  const deepseek = template("deepseek");
  const zhipu = template("zhipu");
  const codingPlan = template("zhipu-coding-plan");
  const draft = createProviderFromTemplate(deepseek, "deepseek");
  const switched = applyProviderTemplate(draft, codingPlan, {
    id: "zhipu-coding-plan",
    name: codingPlan.name,
  });

  assertEqual(switched.id, "zhipu-coding-plan");
  assertEqual(switched.name, "Zhipu Coding Plan");
  assertEqual(switched.templateKind, "zhipu-coding-plan");
  assertEqual(switched.baseUrl, "https://open.bigmodel.cn/api/coding/paas/v4");
  assertEqual(switched.providerKind, "zhipu");
  assertEqual(switched.defaultModel, "glm-5.2");
  assertDeepEqual(
    switched.defaultModels.map((model) => model.slug),
    zhipu.defaultModels.map((model) => model.slug),
  );
}

function zhipuProviderDraftUsesUniqueKey() {
  const zhipu = template("zhipu");
  const existing = [
    createProviderFromTemplate(zhipu, "zhipu"),
    createProviderFromTemplate(zhipu, "zhipu-2"),
  ];
  const nextId = suggestProviderId(existing, zhipu.id);
  const draft = createProviderFromTemplate(zhipu, nextId);

  assertEqual(nextId, "zhipu-3");
  assertEqual(draft.id, "zhipu-3");
  assertEqual(draft.templateKind, "zhipu");
  assertEqual(draft.defaultModel, "glm-5.2");
}

function zhipuCodingPlanProviderDraftUsesUniqueKey() {
  const codingPlan = template("zhipu-coding-plan");
  const existing = [
    createProviderFromTemplate(codingPlan, "zhipu-coding-plan"),
    createProviderFromTemplate(codingPlan, "zhipu-coding-plan-2"),
  ];
  const nextId = suggestProviderId(existing, codingPlan.id);
  const draft = createProviderFromTemplate(codingPlan, nextId);

  assertEqual(nextId, "zhipu-coding-plan-3");
  assertEqual(draft.id, "zhipu-coding-plan-3");
  assertEqual(draft.templateKind, "zhipu-coding-plan");
  assertEqual(draft.defaultModel, "glm-5.2");
}

function editingProviderDraftDoesNotMutateProviderList() {
  const deepseek = template("deepseek");
  const openai = template("openai");
  const providers: ProviderRecord[] = [createProviderFromTemplate(deepseek, "deepseek")];
  const before = JSON.stringify(providers);
  const draft = applyProviderTemplate(cloneProvider(providers[0]), openai, {
    id: "openai",
    name: openai.name,
  });

  assertEqual(draft.templateKind, "openai");
  assertEqual(JSON.stringify(providers), before);
  assertEqual(providers[0].templateKind, "deepseek");
}

function roleChangeProducesCompleteNormalizedSnapshot() {
  const deepseekProvider = createProviderFromTemplate(template("deepseek"), "deepseek");
  const openaiProvider = createProviderFromTemplate(template("openai"), "openai");
  const roles: RoleRecord[] = [
    {
      key: "planner",
      displayName: "Planner",
      provider: "openai",
      model: openaiProvider.defaultModel,
      effort: "medium",
    },
  ];
  const normalized = normalizeRolesForProviders(roles, [deepseekProvider, openaiProvider]);

  assertDeepEqual(
    normalized.map((role) => role.key),
    ["explorer", "planner", "executor", "reviewer"],
  );
  assertEqual(normalized.find((role) => role.key === "planner")?.provider, "openai");
  assertEqual(normalized.find((role) => role.key === "explorer")?.provider, "deepseek");
}

function selectedContextWindowFollowsPlannerModel() {
  const deepseekProvider = createProviderFromTemplate(template("deepseek"), "deepseek");
  const openaiProvider = createProviderFromTemplate(template("openai"), "openai");
  const roles: RoleRecord[] = [
    {
      key: "planner",
      displayName: "Planner",
      provider: "openai",
      model: "gpt-5.4-mini",
      effort: "medium",
    },
  ];

  assertEqual(
    selectedContextWindow([deepseekProvider, openaiProvider], roles, runtime),
    272_000,
  );
}

function markdownRendersGfmTable() {
  const html = renderMarkdown("| 检查项 | 状态 |\n|--------|------|\n| TypeScript | ✅ 通过 |");

  assertIncludes(html, "<table>");
  assertIncludes(html, "<th>检查项</th>");
  assertIncludes(html, "<td>TypeScript</td>");
}

function markdownRendersCodeBlocksAndInlineCode() {
  const html = renderMarkdown("使用 `MarkdownContent`。\n\n```ts\nconst ok = true;\n```");

  assertIncludes(html, "<code>MarkdownContent</code>");
  assertIncludes(html, "<pre><code class=\"language-ts\">const ok = true;</code></pre>");
}

function markdownAllowsOnlySafeLinks() {
  const html = renderMarkdown("[安全](https://example.com) [危险](javascript:alert(1))");

  assertIncludes(html, "href=\"https://example.com\"");
  assertNotIncludes(html, "href=\"javascript:alert(1)\"");
}

function markdownEscapesHtmlTokens() {
  const html = renderMarkdown("<script>alert(1)</script>");

  assertIncludes(html, "&lt;script&gt;alert(1)&lt;/script&gt;");
  assertNotIncludes(html, "<script>");
}

markdownRendersGfmTable();
markdownRendersCodeBlocksAndInlineCode();
markdownAllowsOnlySafeLinks();
markdownEscapesHtmlTokens();
staleTimelineLoadKeepsNewTurnItems();
sessionStateProjectionSnapshotRestoresTimelineWithoutEvents();
sessionStateEventsRestoreDurableStatusSnapshots();
freshTimelineLoadMayReplaceSnapshot();
staleTimelineLoadDoesNotOverwriteLiveDelta();
staleSessionStateDoesNotReplayOldStatusEvents();
toolArgumentDeltaRequiresSnapshot();
toolResultDeltaRequiresSnapshot();
textDeltaRequiresSnapshotForCommentaryChannel();
turnCompletedWithNoPartsDoesNotDeleteLiveContent();
staleEventDoesNotAdvanceCursor();
runPromptFailedKeepsFailedStatusText();
userInputRequestStoresPendingComposerState();
userInputResolvedClearsPendingComposerState();
completedSnapshotKeepsFirstItemSequence();
terminalSnapshotClearsLiveDeltaOverlay();
realtimeAndHistoricalTimelineEventsConverge();
consecutiveToolsCollapseIntoToolGroup();
thinkingDoesNotBreakToolGroup();
inferenceDoesNotBreakToolGroup();
repeatedThinkingItemsCollapseIntoOneThought();
thinkingFromDifferentTurnsDoesNotMerge();
assistantTextBreaksToolGroup();
planEntryBreaksToolGroupAndRendersAsPlan();
livePlanDeltaCreatesPlanEntry();
interactionEventStoresPlanConfirmation();
messagePartUpdatedWithoutInteractionDoesNotInferPlanConfirmation();
planLifecycleLoadedKeepsInteractionStateSeparate();
sessionStateLoadedPlanStateAnnotatesPlanWithoutOpeningComposer();
sessionStateLoadedNewPlanStatesAnnotatePlan();
historicalTimelineLoadDoesNotCreatePlanAction();
laterRunWithoutPlanDoesNotReopenHistoricalPlan();
interactionEventKeepsLiveInteractionByCurrentRun();
freshContextRunSwitchesToReturnedSession();
planImplementationSubmittedShowsWaitingWithoutSwitchingSession();
sessionHandoffStartedSwitchesBeforeLiveEvents();
studioTurnEventShowsWaitingState();
studioTimelineEventClearsWaitingAndStreamsContent();
studioTimelineEventUsesEnvelopeSequenceForCursor();
studioHandoffEventSwitchesToTargetSession();
studioSessionRuntimeEventUpdatesActiveSkills();
liveEventsForTargetAreIgnoredBeforeHandoffStarted();
sessionSelectionDedupesReturnedSessions();
sessionModeUpdateNoLongerSwitchesForFreshContextRun();
promptSubmittedCreatesOptimisticTimelineFeedback();
modelTimelineEventClearsWaitingFeedback();
userTimelineEventKeepsWaitingFeedback();
inferenceStartKeepsWaitingFeedback();
assistantSnapshotClearsOptimisticWaitingFeedback();
runPromptFailedClearsWaitingFeedback();
thinkingEntriesExposeStreamingStatusAndDuration();
completedThinkingEntryUsesDuration();
liveCompletedPlanDoesNotCreatePlanInteraction();
sessionSelectionClearsInteractionState();
selectingCurrentSessionKeepsTimelineState();
toolsFromDifferentTurnsDoNotMerge();
toolGroupStatusUsesPriority();
sessionModeUpdateKeepsTimelineAndUpdatesSessions();
projectSelectionLoadedAfterSessionDeleteClearsSelectedSession();
projectSelectionLoadedCanClearSelectedProject();
configLoadedUpdatesPermissionMode();
providerUsagesLoadedDoesNotReplaceConfigState();
mcpHealthUpdatedRefreshesMcpServersAndRuntime();
lspHealthUpdatedRefreshesLspServersAndRuntime();
applyPatchCountsFilesFromResultSummary();
skillActivationEventUsesSessionRuntimePayload();
skillViewToolResultJsonNoLongerUpdatesActiveSkills();
skillActivationFromOtherSessionDoesNotUpdateActiveSkills();
unknownToolsUseFallbackAndKeepDetails();
quietFileToolFailureKeepsResultVisible();
inferenceAndNormalTurnTraceAreHidden();
abnormalTurnTraceIsKeptWithContent();
liveFailedTimelinePartViewKeepsErrorMessage();
providerDraftUsesSingleAddEntryAndUniqueKey();
providerDraftTemplateSwitchUpdatesTemplateFields();
openAiTemplateUsesCodexModelMetadata();
providerDraftTemplateSwitchSupportsZhipu();
providerDraftTemplateSwitchSupportsZhipuCodingPlan();
zhipuProviderDraftUsesUniqueKey();
zhipuCodingPlanProviderDraftUsesUniqueKey();
editingProviderDraftDoesNotMutateProviderList();
roleChangeProducesCompleteNormalizedSnapshot();
selectedContextWindowFollowsPlannerModel();




