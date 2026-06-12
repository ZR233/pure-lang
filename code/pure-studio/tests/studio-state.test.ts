import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server.browser";
import { initialStudioState, studioReducer } from "../src/state/studio-state";
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
  RunPromptResponse,
  SessionRuntime,
  TimelineItem,
  TimelineItemDeltaEvent,
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

function textItem(itemId: string, sequence: number, content: string): TimelineItem {
  return {
    turnId: itemId.split("-").slice(0, 2).join("-") || "turn",
    itemId,
    sequence,
    kind: "text",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    role: "assistant",
    content,
    thinkingChunks: [],
  };
}

function planItem(itemId: string, turnId: string, sequence: number, content: string): TimelineItem {
  return {
    turnId,
    itemId,
    sequence,
    kind: "plan",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    role: null,
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

function thinkingItem(itemId: string, turnId: string, sequence: number, content: string): TimelineItem {
  return {
    turnId,
    itemId,
    sequence,
    kind: "thinking",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    role: null,
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
): TimelineItem {
  return {
    turnId,
    itemId,
    sequence,
    kind: "tool",
    status,
    createdAt: sequence,
    updatedAt: sequence,
    role: null,
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
): TimelineItem {
  return {
    turnId,
    itemId,
    sequence,
    kind: "turn",
    status,
    createdAt: sequence,
    updatedAt: sequence,
    role: null,
    content,
    thinkingChunks: [],
  };
}

function inferenceItem(itemId: string, turnId: string, sequence: number): TimelineItem {
  return {
    turnId,
    itemId,
    sequence,
    kind: "inference",
    status: "completed",
    createdAt: sequence,
    updatedAt: sequence,
    role: null,
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

function response(timelineItems: TimelineItem[]): RunPromptResponse {
  const timelineNextSequence =
    timelineItems.reduce((max, item) => Math.max(max, item.sequence), -1) + 1;
  return {
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
    ],
    agentEvents: [],
    agents: [],
    sessionRuntime: runtime,
    timelineItems,
    interactions: [],
    timelineNextSequence,
    turnStatus: "completed",
    turnAbortReason: null,
    turnError: null,
  };
}

function responseWithSequence(
  timelineItems: TimelineItem[],
  timelineNextSequence: number,
): RunPromptResponse {
  return {
    ...response(timelineItems),
    timelineNextSequence,
  };
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

function entriesForTimeline(items: TimelineItem[]) {
  const loaded = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items,
    nextSequence: items.reduce((max, item) => Math.max(max, item.sequence), -1) + 1,
  });
  return selectTimelineEntries(loaded);
}

function staleTimelineLoadKeepsNewTurnItems() {
  const oldItem = textItem("turn-1-text", 1, "old");
  const newItem = textItem("turn-2-text", 10, "new");
  const withNewTurn = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([newItem]),
    status: "done",
  });

  const afterStaleLoad = studioReducer(withNewTurn, {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [oldItem],
    nextSequence: 2,
  });

  assertDeepEqual(afterStaleLoad.timelineOrder, ["turn-1-text", "turn-2-text"]);
  assertEqual(afterStaleLoad.timelineItems.get("turn-2-text")?.content, "new");
  assertEqual(afterStaleLoad.timelineNextSequence, 11);
}

function freshTimelineLoadMayReplaceSnapshot() {
  const firstItem = textItem("turn-1-text", 1, "first");
  const replacement = textItem("turn-1-text", 2, "replacement");
  const loaded = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [firstItem],
    nextSequence: 2,
  });

  const refreshed = studioReducer(loaded, {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [replacement],
    nextSequence: 3,
  });

  assertDeepEqual(refreshed.timelineOrder, ["turn-1-text"]);
  assertEqual(refreshed.timelineItems.get("turn-1-text")?.content, "replacement");
  assertEqual(refreshed.timelineNextSequence, 3);
}

function staleTimelineLoadDoesNotOverwriteLiveDelta() {
  const oldItem = textItem("turn-1-text", 1, "old");
  const started = {
    ...textItem("turn-2-text", 10, ""),
    status: "streaming" as const,
  };
  const delta: TimelineItemDeltaEvent = {
    turnId: "turn-2",
    itemId: "turn-2-text",
    sequence: 11,
    kind: "text",
    status: "streaming",
    createdAt: 10,
    updatedAt: 11,
    delta: { type: "text", delta: "new" },
  };
  const completed = textItem("turn-2-text", 12, "new");
  const liveStarted = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: started } },
    statusText: "running",
  });
  const liveDelta = studioReducer(liveStarted, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemDelta: { event: delta } },
    statusText: "running",
  });
  const liveCompleted = studioReducer(liveDelta, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 12, item: completed } },
    statusText: "done",
  });

  const afterStaleLoad = studioReducer(liveCompleted, {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [oldItem],
    nextSequence: 2,
  });

  assertDeepEqual(afterStaleLoad.timelineOrder, ["turn-1-text", "turn-2-text"]);
  assertEqual(afterStaleLoad.timelineItems.get("turn-2-text")?.content, "new");
  assertEqual(afterStaleLoad.timelineNextSequence, 13);
}

function toolArgumentDeltaBeforeStartIsPreserved() {
  const delta: TimelineItemDeltaEvent = {
    turnId: "turn-1",
    itemId: "turn-1-call-1",
    sequence: 10,
    kind: "tool",
    status: "streaming",
    createdAt: 10,
    updatedAt: 10,
    delta: { type: "toolArguments", delta: "{\"path\":\"a.ts\"" },
  };
  const started = toolItem("turn-1-call-1", "turn-1", 9, "read_file", "");
  started.status = "streaming";
  const completed = toolItem("turn-1-call-1", "turn-1", 11, "read_file", "");
  const withDelta = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemDelta: { event: delta } },
    statusText: "running",
  });
  const withStart = studioReducer(withDelta, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: started } },
    statusText: "running",
  });
  const withCompleted = studioReducer(withStart, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 11, item: completed } },
    statusText: "done",
  });

  const tool = withCompleted.timelineItems.get("turn-1-call-1")?.tool;
  assertEqual(tool?.name, "read_file");
  assertEqual(tool?.arguments, "{\"path\":\"a.ts\"");
}

function toolResultDeltaBeforeStartIsPreserved() {
  const delta: TimelineItemDeltaEvent = {
    turnId: "turn-1",
    itemId: "turn-1-call-1",
    sequence: 10,
    kind: "tool",
    status: "streaming",
    createdAt: 10,
    updatedAt: 10,
    delta: { type: "toolResult", delta: "partial result" },
  };
  const completed = toolItem("turn-1-call-1", "turn-1", 11, "read_file", { path: "a.ts" });
  const withDelta = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemDelta: { event: delta } },
    statusText: "running",
  });
  const withCompleted = studioReducer(withDelta, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 11, item: completed } },
    statusText: "done",
  });

  const tool = withCompleted.timelineItems.get("turn-1-call-1")?.tool;
  assertEqual(tool?.name, "read_file");
  assertEqual(tool?.arguments, "{\"path\":\"a.ts\"}");
  assertEqual(tool?.result, "partial result");
}

function runPromptLoadedWithEmptyItemsDoesNotDeleteLiveContent() {
  const liveItem = textItem("turn-2-text", 10, "live");
  const liveState = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 10, item: liveItem } },
    statusText: "done",
  });

  const completed = studioReducer(liveState, {
    type: "runPromptLoaded",
    payload: responseWithSequence([], 20),
    status: "done",
  });

  assertDeepEqual(completed.timelineOrder, ["turn-2-text"]);
  assertEqual(completed.timelineItems.get("turn-2-text")?.content, "live");
  assertEqual(completed.timelineNextSequence, 20);
}

function runPromptLoadedErroredKeepsFailedStatusText() {
  const failedResponse: RunPromptResponse = {
    ...response([turnItem("turn-1-turn", "turn-1", 3, "failed", "provider error")]),
    turnStatus: "errored",
    turnAbortReason: "providerError",
    turnError: "provider error",
  };

  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: failedResponse,
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
  const completed = textItem("turn-2-text", 12, "final");
  const liveStarted = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: started } },
    statusText: "running",
  });

  const liveCompleted = studioReducer(liveStarted, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 12, item: completed } },
    statusText: "done",
  });

  assertDeepEqual(liveCompleted.timelineOrder, ["turn-2-text"]);
  assertEqual(liveCompleted.timelineItems.get("turn-2-text")?.sequence, 10);
  assertEqual(liveCompleted.timelineItems.get("turn-2-text")?.content, "final");
  assertEqual(liveCompleted.timelineNextSequence, 13);
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
  const delta: TimelineItemDeltaEvent = {
    turnId: "turn-1",
    itemId: "turn-1-plan",
    sequence: 11,
    kind: "plan",
    status: "streaming",
    createdAt: 10,
    updatedAt: 11,
    delta: { type: "plan", delta: "1. Inspect\n" },
  };
  const liveStarted = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: started } },
    statusText: "running",
  });
  const liveDelta = studioReducer(liveStarted, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemDelta: { event: delta } },
    statusText: "running",
  });
  const entries = selectTimelineEntries(liveDelta);

  assertDeepEqual(entries.map((entry) => entry.kind), ["plan"]);
  const plan = entries[0];
  if (plan?.kind !== "plan") {
    throw new Error(`Expected plan entry, got ${plan?.kind}`);
  }
  assertEqual(plan.content, "1. Inspect\n");
  assertEqual(liveDelta.timelineItems.get("turn-1-plan")?.content, "1. Inspect\n");
  assertEqual(liveDelta.activeInteractionId, null);
}

function runPromptLoadedUsesBackendPlanConfirmationInteraction() {
  const interaction = planConfirmationInteraction();
  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: {
      ...response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
      interactions: [interaction],
    },
    status: "done",
  });

  assertEqual(state.activeInteractionId, interaction.interactionId);
  assertDeepEqual(state.interactions.get(interaction.interactionId), interaction);
}

function runPromptLoadedWithoutInteractionDoesNotInferPlanConfirmation() {
  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });

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
    timelineNextSequence: 12,
    status: "ready",
  });

  assertEqual(dismissed.activeInteractionId, interaction.interactionId);
  assertEqual(dismissed.planStates.get("turn-1-plan")?.state, "dismissed");
  assertEqual(dismissed.timelineNextSequence, 12);
}

function timelineLoadedPlanStateAnnotatesPlanWithoutOpeningComposer() {
  const state = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    planStates: [planState("turn-1-plan", "dismissed")],
    nextSequence: 13,
  });
  const entries = selectTimelineEntries(state);
  const plan = entries.find((entry) => entry.kind === "plan");

  assertEqual(state.activeInteractionId, null);
  assertEqual(plan?.kind, "plan");
  if (plan?.kind !== "plan") {
    throw new Error("expected plan entry");
  }
  assertEqual(plan.planState?.state, "dismissed");
}

function timelineLoadedNewPlanStatesAnnotatePlan() {
  for (const state of ["pendingConfirmation", "continuedPlanning", "cancelled"] as const) {
    const reduced = studioReducer(selectedState(), {
      type: "timelineLoaded",
      sessionId: "session-1",
      items: [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
      planStates: [planState("turn-1-plan", state)],
      nextSequence: 13,
    });
    const plan = selectTimelineEntries(reduced).find((entry) => entry.kind === "plan");

    if (plan?.kind !== "plan") {
      throw new Error("expected plan entry");
    }
    assertEqual(plan.planState?.state, state);
  }
}

function historicalTimelineLoadDoesNotCreatePlanAction() {
  const state = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    nextSequence: 11,
  });

  assertEqual(state.activeInteractionId, null);
}

function laterRunWithoutPlanDoesNotReopenHistoricalPlan() {
  const withHistory = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    nextSequence: 11,
  });
  const submitted = studioReducer(withHistory, {
    type: "promptSubmitted",
    status: "running",
    startedAt: 100,
    prompt: "continue",
  });
  const completed = studioReducer(submitted, {
    type: "runPromptLoaded",
    payload: response([textItem("turn-2-text", 20, "continue")]),
    status: "done",
  });

  assertEqual(completed.activeInteractionId, null);
}

function runPromptLoadedKeepsLiveInteractionByCurrentRun() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 100,
    prompt: "make a plan",
  });
  const interaction = planConfirmationInteraction("turn-2-plan", "1. Inspect");
  const completed = studioReducer(submitted, {
    type: "runPromptLoaded",
    payload: {
      ...responseWithSequence([], 13),
      interactions: [interaction],
    },
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
  const freshRun = response([textItem("turn-2-text", 20, "implemented")]);
  freshRun.sessionId = "session-2";
  freshRun.sessionRuntime = { ...runtime, sessionId: "session-2" };
  freshRun.sessions = [
    {
      id: "session-2",
      projectId: "project-1",
      title: "Implementation",
      mode: "auto",
      updatedAt: 3,
      visibility: "active",
    },
  ];

  const completed = studioReducer(resolving, {
    type: "runPromptLoaded",
    payload: freshRun,
    status: "done",
  });

  assertEqual(completed.selectedSessionId, "session-2");
  assertEqual(completed.timelineItems.get("turn-2-text")?.content, "implemented");
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

function runPromptLoadedDedupesReturnedSessions() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 100,
    prompt: "implement",
  });
  const run = response([textItem("turn-2-text", 20, "implemented")]);
  run.sessions = [run.sessions[0]!, { ...run.sessions[0]!, title: "Duplicate" }];

  const completed = studioReducer(submitted, {
    type: "runPromptLoaded",
    payload: run,
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
  assertDeepEqual(state.timelineOrder, ["optimistic-user-1234", "optimistic-waiting-1234"]);
  assertEqual(state.timelineNextSequence, before.timelineNextSequence);
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
  const updated = studioReducer(submitted, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: realItem } },
    statusText: "running",
  });

  assertDeepEqual(updated.timelineOrder, ["optimistic-user-1234", "turn-1-text"]);
  assertEqual(updated.timelineItems.has("optimistic-user-1234"), true);
  assertEqual(updated.timelineItems.has("optimistic-waiting-1234"), false);
}

function userTimelineEventKeepsWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const realUser = textItem("turn-1-user", submitted.timelineNextSequence, "Build the thing");
  realUser.role = "user";
  const updated = studioReducer(submitted, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: realUser } },
    statusText: "running",
  });
  const entries = selectTimelineEntries(updated);

  assertDeepEqual(entries.map((entry) => entry.kind), ["message", "status"]);
  const message = entries[0];
  const status = entries[1];
  if (message?.kind !== "message" || status?.kind !== "status") {
    throw new Error("Expected real user message followed by waiting status");
  }
  assertEqual(message.content, "Build the thing");
  assertEqual(status.content, "waitingForModel");
  assertEqual(updated.timelineItems.has("optimistic-user-1234"), false);
  assertEqual(updated.timelineItems.has("optimistic-waiting-1234"), true);
  assertDeepEqual(updated.timelineOrder, ["turn-1-user", "optimistic-waiting-1234"]);
}

function inferenceStartKeepsWaitingFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const inference: TimelineItem = {
    turnId: "turn-1",
    itemId: "turn-1-inf-0",
    sequence: 2,
    kind: "inference",
    status: "running",
    createdAt: 2,
    updatedAt: 2,
    role: null,
    content: "",
    thinkingChunks: [],
    inference: {
      inferenceId: "turn-1-inf-0",
      model: "model-a",
    },
  };
  const updated = studioReducer(submitted, {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemStarted: { item: inference } },
    statusText: "running",
  });

  assertEqual(updated.timelineItems.has("optimistic-waiting-1234"), true);
}

function runPromptLoadedClearsOptimisticTimelineFeedback() {
  const submitted = studioReducer(selectedState(), {
    type: "promptSubmitted",
    status: "running",
    startedAt: 1234,
    prompt: "Build the thing",
  });
  const loaded = studioReducer(submitted, {
    type: "runPromptLoaded",
    payload: response([textItem("turn-1-text", 10, "Done")]),
    status: "done",
  });

  assertDeepEqual(loaded.timelineOrder, ["turn-1-text"]);
  assertEqual(loaded.timelineItems.has("optimistic-user-1234"), false);
  assertEqual(loaded.timelineItems.has("optimistic-waiting-1234"), false);
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
  assertEqual(failed.timelineItems.has("optimistic-user-1234"), true);
  assertEqual(failed.timelineItems.has("optimistic-waiting-1234"), false);
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
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 10, item } },
    statusText: "running",
  });

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
  const loaded = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [textItem("turn-1-text", 10, "hello")],
    nextSequence: 11,
  });
  const withPlan = studioReducer(loaded, {
    type: "runPromptLoaded",
    payload: {
      ...response([planItem("turn-1-plan", "turn-1", 12, "1. Inspect")]),
      interactions: [planConfirmationInteraction()],
    },
    status: "done",
  });
  const selectedAgain = studioReducer(withPlan, {
    type: "sessionSelectionLoaded",
    status: "loaded",
    payload: {
      sessionId: "session-1",
      sessions: withPlan.sessions,
      agentEvents: [],
      agents: [],
      interactions: [planConfirmationInteraction()],
      sessionRuntime: runtime,
    },
  });

  assertDeepEqual(selectedAgain.timelineOrder, withPlan.timelineOrder);
  assertEqual(selectedAgain.timelineItems.get("turn-1-plan")?.content, "1. Inspect");
  assertEqual(selectedAgain.activeInteractionId, withPlan.activeInteractionId);
  assertEqual(selectedAgain.timelineNextSequence, withPlan.timelineNextSequence);
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
  const liveState = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 10, item: liveItem } },
    statusText: "running",
  });

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
  assertDeepEqual(updated.timelineOrder, ["turn-1-plan"]);
  assertEqual(updated.timelineItems.get("turn-1-plan")?.content, "1. Inspect");
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
  const liveState = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [liveItem],
    nextSequence: 11,
  });
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
  assertDeepEqual(updated.timelineOrder, []);
  assertEqual(updated.sessionRuntime, null);
  assertEqual(updated.status, "deleted");
}

function projectSelectionLoadedCanClearSelectedProject() {
  const liveState = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [textItem("turn-1-text", 10, "hello")],
    nextSequence: 11,
  });
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
  assertDeepEqual(updated.timelineOrder, []);
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
    type: "agentEvent",
    sessionId: "session-1",
    sessionRuntime: {
      ...runtime,
      activeSkills: ["openai-docs", "skill-creator"],
      updatedAt: 2,
    },
    event: {
      skillActivated: {
        activation: {
          name: "skill-creator",
          source: "user",
          path: "C:/skills/skill-creator",
          turnId: "turn-1",
          toolCallId: "call-1",
          activatedAt: 10,
        },
      },
    },
    statusText: "running",
  });

  assertDeepEqual(state.sessionRuntime?.activeSkills, ["openai-docs", "skill-creator"]);
}

function skillViewToolResultJsonNoLongerUpdatesActiveSkills() {
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: {
      timelineItemCompleted: {
        sequence: 10,
        item: toolItem(
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
        ),
      },
    },
    statusText: "running",
  });

  assertDeepEqual(state.sessionRuntime?.activeSkills, []);
}

function skillActivationFromOtherSessionDoesNotUpdateActiveSkills() {
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-2",
    sessionRuntime: {
      ...runtime,
      activeSkills: ["skill-creator"],
      updatedAt: 2,
    },
    event: {
      skillActivated: {
        activation: {
          name: "skill-creator",
          source: "user",
          path: "C:/skills/skill-creator",
          turnId: "turn-1",
          toolCallId: "call-1",
          activatedAt: 10,
        },
      },
    },
    statusText: "running",
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

function liveFailedTimelineItemKeepsErrorMessage() {
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: {
      timelineItemFailed: {
        sequence: 10,
        item: turnItem("turn-1-turn", "turn-1", 10, "failed"),
        error: "LLM provider error: missing API key",
      },
    },
    statusText: "error",
  });

  const entries = selectTimelineEntries(state);

  assertEqual(state.timelineItems.get("turn-1-turn")?.content, "LLM provider error: missing API key");
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
  const gpt52 = openai.defaultModels.find((model) => model.slug === "gpt-5.2");

  assertDeepEqual(
    openai.defaultModels.map((model) => model.slug),
    ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex", "gpt-5.2"],
  );
  assertEqual(gpt55?.contextWindow, 272_000);
  assertEqual(gpt55?.maxContextWindow, 272_000);
  assertEqual(gpt55?.maxOutputTokens, null);
  assertEqual(gpt55?.currency ?? null, null);
  assertDeepEqual(gpt55?.reasoningEfforts, ["medium", "low", "high", "xhigh"]);
  assertEqual(gpt54?.maxContextWindow, 1_000_000);
  assertEqual(gpt52?.truncationMode, "bytes");
  assertEqual(gpt52?.maxOutputTokens, null);
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
  assertEqual(switched.defaultModel, "glm-5.1");
  assertDeepEqual(
    switched.defaultModels.map((model) => model.slug),
    ["glm-5.1", "glm-5", "glm-5-turbo", "glm-4.7", "glm-4.7-flashx", "glm-4.7-flash"],
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
  assertEqual(switched.defaultModel, "glm-5.1");
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
  assertEqual(draft.defaultModel, "glm-5.1");
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
  assertEqual(draft.defaultModel, "glm-5.1");
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
freshTimelineLoadMayReplaceSnapshot();
staleTimelineLoadDoesNotOverwriteLiveDelta();
toolArgumentDeltaBeforeStartIsPreserved();
toolResultDeltaBeforeStartIsPreserved();
runPromptLoadedWithEmptyItemsDoesNotDeleteLiveContent();
runPromptLoadedErroredKeepsFailedStatusText();
userInputRequestStoresPendingComposerState();
userInputResolvedClearsPendingComposerState();
completedSnapshotKeepsFirstItemSequence();
consecutiveToolsCollapseIntoToolGroup();
thinkingDoesNotBreakToolGroup();
inferenceDoesNotBreakToolGroup();
repeatedThinkingItemsCollapseIntoOneThought();
thinkingFromDifferentTurnsDoesNotMerge();
assistantTextBreaksToolGroup();
planEntryBreaksToolGroupAndRendersAsPlan();
livePlanDeltaCreatesPlanEntry();
runPromptLoadedUsesBackendPlanConfirmationInteraction();
runPromptLoadedWithoutInteractionDoesNotInferPlanConfirmation();
planLifecycleLoadedKeepsInteractionStateSeparate();
timelineLoadedPlanStateAnnotatesPlanWithoutOpeningComposer();
timelineLoadedNewPlanStatesAnnotatePlan();
historicalTimelineLoadDoesNotCreatePlanAction();
laterRunWithoutPlanDoesNotReopenHistoricalPlan();
runPromptLoadedKeepsLiveInteractionByCurrentRun();
freshContextRunSwitchesToReturnedSession();
planImplementationSubmittedShowsWaitingWithoutSwitchingSession();
runPromptLoadedDedupesReturnedSessions();
sessionModeUpdateNoLongerSwitchesForFreshContextRun();
promptSubmittedCreatesOptimisticTimelineFeedback();
modelTimelineEventClearsWaitingFeedback();
userTimelineEventKeepsWaitingFeedback();
inferenceStartKeepsWaitingFeedback();
runPromptLoadedClearsOptimisticTimelineFeedback();
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
liveFailedTimelineItemKeepsErrorMessage();
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
