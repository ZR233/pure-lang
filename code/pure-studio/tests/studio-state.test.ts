import { initialStudioState, studioReducer } from "../src/state/studio-state";
import type {
  ConfigPayload,
  RunPromptResponse,
  SessionRuntime,
  TimelineItem,
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
    turnId: itemId.split("-")[0] ?? "turn",
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
    turnStatus: "completed",
    turnAbortReason: null,
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
  });

  assertDeepEqual(afterStaleLoad.timelineOrder, ["turn-1-text", "turn-2-text"]);
  assertEqual(afterStaleLoad.timelineItems.get("turn-2-text")?.content, "new");
  assertEqual(afterStaleLoad.timelineMaxSequence, 10);
}

function freshTimelineLoadMayReplaceSnapshot() {
  const firstItem = textItem("turn-1-text", 1, "first");
  const replacement = textItem("turn-1-text", 2, "replacement");
  const loaded = studioReducer(selectedState(), {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [firstItem],
  });

  const refreshed = studioReducer(loaded, {
    type: "timelineLoaded",
    sessionId: "session-1",
    items: [replacement],
  });

  assertDeepEqual(refreshed.timelineOrder, ["turn-1-text"]);
  assertEqual(refreshed.timelineItems.get("turn-1-text")?.content, "replacement");
  assertEqual(refreshed.timelineMaxSequence, 2);
}

staleTimelineLoadKeepsNewTurnItems();
freshTimelineLoadMayReplaceSnapshot();
