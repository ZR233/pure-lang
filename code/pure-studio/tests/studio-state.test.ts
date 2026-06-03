import { initialStudioState, studioReducer } from "../src/state/studio-state";
import type {
  ConfigPayload,
  RunPromptResponse,
  SessionRuntime,
  TimelineItem,
  TimelineItemDeltaEvent,
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

staleTimelineLoadKeepsNewTurnItems();
freshTimelineLoadMayReplaceSnapshot();
staleTimelineLoadDoesNotOverwriteLiveDelta();
runPromptLoadedWithEmptyItemsDoesNotDeleteLiveContent();
completedSnapshotKeepsFirstItemSequence();
