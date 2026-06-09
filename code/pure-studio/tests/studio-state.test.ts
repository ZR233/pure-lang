import { initialStudioState, studioReducer } from "../src/state/studio-state";
import { selectTimelineEntries } from "../src/state/selectors";
import { normalizeRolesForProviders } from "../src/components/RoleSettings";
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
  ProviderRecord,
  PlanState,
  RoleRecord,
  RunPromptResponse,
  SessionRuntime,
  TimelineItem,
  TimelineItemDeltaEvent,
  ToolCallStatus2,
  UserInputRequest,
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

const config: ConfigPayload = {
  toml: "",
  permissionMode: "request-approval",
  providers: [],
  roles: [],
  templates: [],
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
  updatedAt: 1,
};

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
      },
    ],
    agentEvents: [],
    agents: [],
    sessionRuntime: runtime,
    timelineItems,
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
        },
      ],
      selectedSessionId: "session-1",
      agentEvents: [],
      agents: [],
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
  const request: UserInputRequest = {
    requestId: "call-ask",
    sessionId: "session-1",
    toolId: "call-ask",
    questions: [
      {
        id: "mode",
        header: "Mode",
        question: "Which mode?",
        options: [{ label: "Fast", description: "Use the fast path." }],
      },
    ],
  };

  const state = studioReducer(selectedState(), {
    type: "userInputRequested",
    payload: request,
    status: "waiting",
  });

  assertEqual(state.turnPhase, "userInput");
  assertDeepEqual(state.pendingUserInput, request);
}

function userInputResolvedClearsPendingComposerState() {
  const request: UserInputRequest = {
    requestId: "call-ask",
    sessionId: "session-1",
    toolId: "call-ask",
    questions: [{ id: "notes", header: "Notes", question: "Anything else?" }],
  };
  const pending = studioReducer(selectedState(), {
    type: "userInputRequested",
    payload: request,
    status: "waiting",
  });

  const resolved = studioReducer(pending, {
    type: "userInputResolved",
    payload: { requestId: "call-ask" },
    status: "answered",
  });

  assertEqual(resolved.pendingUserInput, null);
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
  assertEqual(liveDelta.planAction, null);
}

function runPromptLoadedPlanCreatesPendingPlanAction() {
  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });

  assertDeepEqual(state.planAction, {
    planId: "turn-1-plan",
    content: "1. Inspect",
    mode: "choice",
  });
}

function runPromptLoadedStreamingPlanCreatesPendingPlanAction() {
  const streamingPlan = {
    ...planItem("turn-1-plan", "turn-1", 10, "1. Inspect"),
    status: "streaming" as const,
  };
  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([streamingPlan]),
    status: "done",
  });

  assertDeepEqual(state.planAction, {
    planId: "turn-1-plan",
    content: "1. Inspect",
    mode: "choice",
  });
}

function runPromptLoadedProposedPlanTextCreatesPendingPlanAction() {
  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([
      textItem(
        "turn-1-text",
        10,
        "好的，计划如下：\n\n<proposed_plan>\n# 修复雨滴效果\n\n## 摘要\n- 调整 canvas 雨滴。\n</proposed_plan>",
      ),
    ]),
    status: "done",
  });

  assertDeepEqual(state.planAction, {
    planId: "turn-1-text",
    content: "# 修复雨滴效果\n\n## 摘要\n- 调整 canvas 雨滴。",
    mode: "choice",
  });
}

function runPromptLoadedPlanStateSuppressesPendingPlanAction() {
  const state = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: {
      ...response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
      planStates: [planState("turn-1-plan", "accepted")],
    },
    status: "done",
  });

  assertEqual(state.planAction, null);
  assertEqual(state.planStates.get("turn-1-plan")?.state, "accepted");
}

function planLifecycleLoadedClearsExistingPlanAction() {
  const withPlan = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });
  const dismissed = studioReducer(withPlan, {
    type: "planLifecycleLoaded",
    sessionId: "session-1",
    planStates: [planState("turn-1-plan", "dismissed")],
    timelineNextSequence: 12,
    status: "ready",
  });

  assertEqual(dismissed.planAction, null);
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

  assertEqual(state.planAction, null);
  assertEqual(plan?.kind, "plan");
  if (plan?.kind !== "plan") {
    throw new Error("expected plan entry");
  }
  assertEqual(plan.planState?.state, "dismissed");
}

function dismissPlanActionPreventsSamePlanFromReopening() {
  const withPlan = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });
  const dismissed = studioReducer(withPlan, { type: "dismissPlanAction" });
  const reloaded = studioReducer(dismissed, {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });

  assertEqual(dismissed.planAction, null);
  assertEqual(dismissed.dismissedPlanId, "turn-1-plan");
  assertEqual(reloaded.planAction, null);
}

function setPlanActionModeUpdatesPendingPlanAction() {
  const withPlan = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });
  const discussing = studioReducer(withPlan, { type: "setPlanActionMode", mode: "discuss" });

  assertEqual(discussing.planAction?.mode, "discuss");
}

function historicalTimelineLoadDoesNotCreatePlanAction() {
  const state = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [planItem("turn-1-plan", "turn-1", 10, "1. Inspect")],
    nextSequence: 11,
  });

  assertEqual(state.planAction, null);
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

  assertEqual(completed.planAction, null);
}

function promptSubmittedCreatesOptimisticTimelineFeedback() {
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
  const realUser = textItem("turn-1-user", 1, "Build the thing");
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

function liveCompletedPlanCreatesPendingPlanAction() {
  const item = planItem("turn-1-plan", "turn-1", 10, "1. Inspect");
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: { timelineItemCompleted: { sequence: 10, item } },
    statusText: "running",
  });

  assertDeepEqual(state.planAction, {
    planId: "turn-1-plan",
    content: "1. Inspect",
    mode: "choice",
  });
}

function sessionSelectionClearsPlanActionState() {
  const withPlan = studioReducer(selectedState(), {
    type: "runPromptLoaded",
    payload: response([planItem("turn-1-plan", "turn-1", 10, "1. Inspect")]),
    status: "done",
  });
  const dismissed = studioReducer(withPlan, { type: "dismissPlanAction" });
  const switched = studioReducer(dismissed, {
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
        },
      ],
      agentEvents: [],
      agents: [],
      sessionRuntime: runtime,
    },
  });

  assertEqual(switched.planAction, null);
  assertEqual(switched.dismissedPlanId, null);
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

function configLoadedUpdatesPermissionMode() {
  const state = studioReducer(selectedState(), {
    type: "configLoaded",
    payload: {
      ...config,
      permissionMode: "auto-review",
    },
    status: "saved",
  });

  assertEqual(state.permissionMode, "auto-review");
  assertEqual(state.status, "saved");
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

function successfulSkillViewImmediatelyUpdatesActiveSkills() {
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

  assertDeepEqual(state.sessionRuntime?.activeSkills, ["skill-creator"]);
}

function repeatedSkillViewDoesNotDuplicateActiveSkills() {
  const existing = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    event: {
      timelineItemCompleted: {
        sequence: 10,
        item: toolItem(
          "turn-1-skill-a",
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

  const repeated = studioReducer(existing, {
    type: "agentEvent",
    sessionId: "session-1",
    event: {
      timelineItemCompleted: {
        sequence: 11,
        item: toolItem(
          "turn-1-skill-b",
          "turn-1",
          11,
          "skill_view",
          { name: "Skill-Creator" },
          "completed",
          JSON.stringify({
            success: true,
            skill: { name: "Skill-Creator" },
            content: "body again",
          }),
        ),
      },
    },
    statusText: "running",
  });

  assertDeepEqual(repeated.sessionRuntime?.activeSkills, ["skill-creator"]);
}

function skillViewAppliesAfterRuntimeSnapshot() {
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-1",
    sessionRuntime: {
      ...runtime,
      activeSkills: ["openai-docs"],
      updatedAt: 2,
    },
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

  assertDeepEqual(state.sessionRuntime?.activeSkills, ["openai-docs", "skill-creator"]);
}

function invalidSkillViewResultsDoNotUpdateActiveSkills() {
  const cases = [
    toolItem(
      "turn-1-skill-failed",
      "turn-1",
      10,
      "skill_view",
      { name: "failed-skill" },
      "completed",
      JSON.stringify({
        success: false,
        skill: { name: "failed-skill" },
      }),
    ),
    toolItem(
      "turn-1-skill-invalid-json",
      "turn-1",
      11,
      "skill_view",
      { name: "bad-json" },
      "completed",
      "not json",
    ),
    toolItem(
      "turn-1-skills-list",
      "turn-1",
      12,
      "skills_list",
      {},
      "completed",
      JSON.stringify({ success: true, skills: [] }),
    ),
  ];
  let state = selectedState();
  for (const item of cases) {
    state = studioReducer(state, {
      type: "agentEvent",
      sessionId: "session-1",
      event: { timelineItemCompleted: { sequence: item.sequence, item } },
      statusText: "running",
    });
  }

  assertDeepEqual(state.sessionRuntime?.activeSkills, []);
}

function skillViewFromOtherSessionDoesNotUpdateActiveSkills() {
  const state = studioReducer(selectedState(), {
    type: "agentEvent",
    sessionId: "session-2",
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

function template(kind: "deepseek" | "openai" | "zhipu") {
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
runPromptLoadedPlanCreatesPendingPlanAction();
runPromptLoadedStreamingPlanCreatesPendingPlanAction();
runPromptLoadedProposedPlanTextCreatesPendingPlanAction();
runPromptLoadedPlanStateSuppressesPendingPlanAction();
planLifecycleLoadedClearsExistingPlanAction();
timelineLoadedPlanStateAnnotatesPlanWithoutOpeningComposer();
dismissPlanActionPreventsSamePlanFromReopening();
setPlanActionModeUpdatesPendingPlanAction();
historicalTimelineLoadDoesNotCreatePlanAction();
laterRunWithoutPlanDoesNotReopenHistoricalPlan();
promptSubmittedCreatesOptimisticTimelineFeedback();
modelTimelineEventClearsWaitingFeedback();
userTimelineEventKeepsWaitingFeedback();
inferenceStartKeepsWaitingFeedback();
runPromptLoadedClearsOptimisticTimelineFeedback();
runPromptFailedClearsWaitingFeedback();
thinkingEntriesExposeStreamingStatusAndDuration();
completedThinkingEntryUsesDuration();
liveCompletedPlanCreatesPendingPlanAction();
sessionSelectionClearsPlanActionState();
toolsFromDifferentTurnsDoNotMerge();
toolGroupStatusUsesPriority();
sessionModeUpdateKeepsTimelineAndUpdatesSessions();
configLoadedUpdatesPermissionMode();
applyPatchCountsFilesFromResultSummary();
successfulSkillViewImmediatelyUpdatesActiveSkills();
repeatedSkillViewDoesNotDuplicateActiveSkills();
skillViewAppliesAfterRuntimeSnapshot();
invalidSkillViewResultsDoNotUpdateActiveSkills();
skillViewFromOtherSessionDoesNotUpdateActiveSkills();
unknownToolsUseFallbackAndKeepDetails();
quietFileToolFailureKeepsResultVisible();
inferenceAndNormalTurnTraceAreHidden();
abnormalTurnTraceIsKeptWithContent();
liveFailedTimelineItemKeepsErrorMessage();
providerDraftUsesSingleAddEntryAndUniqueKey();
providerDraftTemplateSwitchUpdatesTemplateFields();
providerDraftTemplateSwitchSupportsZhipu();
zhipuProviderDraftUsesUniqueKey();
editingProviderDraftDoesNotMutateProviderList();
roleChangeProducesCompleteNormalizedSnapshot();
