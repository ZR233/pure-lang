import { constructTimelineRows, reuseTimelineRows, timelineRowKey } from "../src/solid/timeline/message-timeline.data";
import { groupParts, reasoningDefaultOpen } from "../src/solid/timeline/message-part.data";
import { readPartText, readPlanContent, readToolArguments, readToolResult, reasoningHeading } from "../src/solid/timeline/message-part-text";
import { applyStudioEventBatch, applyStudioProjection, type MessageStore } from "../src/solid/studio-store";
import { selectedSessionView, visibleProjectSessions } from "../src/solid/studio-selectors";
import { streamMarkdown } from "../src/solid/markdown-stream";
import {
  activeInteractionPhase,
  buildPlanConfirmationResolution,
  buildToolApprovalResolution,
  buildUserInputResolution,
  interactionTitle,
  prettyJson,
} from "../src/solid/interaction/interaction-resolution";
import { activeWaitingPhase } from "../src/solid/status/session-status-bar";
import {
  instructionsDraft,
  instructionsInput,
  normalizeMcpServerInput,
  normalizeRolesForProviders,
} from "../src/solid/settings/settings-data";
import { defaultTextCapabilities } from "../src/lib/provider-mapper";
import type {
  AgentDto,
  InteractionRequest,
  McpServerRecord,
  ModelRecord,
  ProviderRecord,
  RoleRecord,
  RuntimeUsage,
  SessionRecord,
  StudioEventEnvelope,
  StudioMessage,
  StudioPart,
  UserQuestion,
} from "../src/types";
import { opencodeTimelineFixtures } from "./fixtures/opencode-message-timeline";

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

function message(id: string, turnId: string, role: StudioMessage["role"], createdAt: number): StudioMessage {
  return {
    messageId: id,
    sessionId: "session-1",
    turnId,
    role,
    status: "streaming",
    createdAt,
    updatedAt: createdAt,
  };
}

function part(
  id: string,
  messageId: string,
  turnId: string,
  order: number,
  partType: StudioPart["partType"],
  text: string,
): StudioPart {
  return {
    partId: id,
    messageId,
    sessionId: "session-1",
    turnId,
    partType,
    order,
    status: "streaming",
    createdAt: order,
    updatedAt: order,
    textChannel: partType === "text" ? "final" : null,
    text,
  };
}

function emptyStore(): MessageStore {
  return {
    projects: [],
    sessions: [],
    selectedProjectId: null,
    selectedSessionId: "session-1",
    providers: [],
    providerTemplates: [],
    providerUsages: [],
    providerUsagesLoading: false,
    providerUsageError: null,
    providerUsageErrors: {},
    providerUsageRefreshing: {},
    providerUsagesLoadedAt: null,
    roles: [],
    instructions: {
      baseOverride: "",
      developer: "",
      user: "",
      projectDocMaxBytes: 65536,
      projectDocFallbackFilenames: [],
    },
    configToml: "",
    configExists: false,
    selectedProviderId: null,
    providerSearch: "",
    activeSettingsTab: "providers",
    permissionMode: "request-approval",
    prompt: "",
    status: "Ready",
    busy: false,
    sessionBusy: {},
    settingsOpen: false,
    mcpServers: [],
    activeMcpServers: [],
    lspServers: [],
    activeLspServers: [],
    turnPhase: {},
    turnStartedAt: {},
    messages: {},
    parts: {},
    partTextAccumDelta: {},
    partDeltaChunks: {},
    messageSequence: {},
    partSequence: {},
    partDeltaSequence: {},
    eventNextSequence: {},
    sessionRuntime: {},
    agents: {},
    agentEvents: {},
    interactions: {},
    activeInteractionId: null,
    activeInteractionPhase: null,
    planStates: {},
  };
}

function event(sequence: number, kind: StudioEventEnvelope["kind"]): StudioEventEnvelope {
  return {
    eventId: `event-${sequence}`,
    sessionId: "session-1",
    sequence,
    createdAt: sequence,
    kind,
  };
}

function interaction(id: string, kind: InteractionRequest["kind"], createdAt: number): InteractionRequest {
  return {
    interactionId: id,
    kind,
    status: "pending",
    scope: {
      sessionId: "session-1",
      turnId: "turn-1",
    },
    payload: kind === "toolApproval"
      ? {
          type: "toolApproval",
          name: "bash",
          arguments: { command: "pwd" },
          workingDirectory: null,
          parentAgentId: null,
        }
      : kind === "userInput"
        ? {
            type: "userInput",
            questions: [{
              id: "choice",
              header: "Choice",
              question: "Pick one",
              isOther: true,
              isSecret: false,
              options: [{ label: "A", description: "First" }],
            }],
          }
        : {
            type: "planConfirmation",
            planId: "plan-1",
            content: "## Plan",
          },
    createdAt,
    updatedAt: createdAt,
  };
}

function testModel(slug: string, efforts: string[]): ModelRecord {
  return {
    slug,
    displayName: slug.toUpperCase(),
    reasoningEfforts: efforts,
    capabilities: defaultTextCapabilities(),
  };
}

function testProvider(id: string, models: ModelRecord[], defaultModel = models[0]?.slug ?? ""): ProviderRecord {
  return {
    id,
    templateKind: "openai",
    name: id,
    subtitle: `${id} Platform`,
    status: "Healthy",
    baseUrl: "https://example.test",
    bearerToken: "",
    hasBearerToken: true,
    defaultModel,
    modelCount: models.length.toString(),
    updatedAt: "now",
    providerKind: "openai-compatible",
    models,
    defaultModels: models,
    customModels: [],
  };
}

function settingsRolesNormalizeAfterProviderOrModelRemoval() {
  const provider = testProvider("current", [testModel("fast", ["low", "high"]), testModel("deep", ["xhigh"])], "fast");
  const roles: RoleRecord[] = [
    { key: "explorer", displayName: "Explorer", provider: "missing", model: "gone", effort: "gone" },
    { key: "planner", displayName: "Planner", provider: "current", model: "deep", effort: "xhigh" },
    { key: "executor", displayName: "Executor", provider: "current", model: "fast", effort: "high" },
    { key: "reviewer", displayName: "Reviewer", provider: "current", model: "fast", effort: "low" },
  ];

  assertDeepEqual(normalizeRolesForProviders(roles, [provider]), [
    { key: "explorer", displayName: "Explorer", provider: "current", model: "fast", effort: "low" },
    { key: "planner", displayName: "Planner", provider: "current", model: "deep", effort: "xhigh" },
    { key: "executor", displayName: "Executor", provider: "current", model: "fast", effort: "high" },
    { key: "reviewer", displayName: "Reviewer", provider: "current", model: "fast", effort: "low" },
  ]);
}

function settingsInstructionsRoundTripDraftAndInput() {
  const draft = instructionsDraft({
    baseOverride: "base",
    developer: "dev",
    user: "user",
    projectDocMaxBytes: 1234,
    projectDocFallbackFilenames: ["PURE.md", "PROJECT.md"],
  });
  assertDeepEqual(draft, {
    baseOverride: "base",
    developer: "dev",
    user: "user",
    projectDocMaxBytes: "1234",
    fallbackFilenames: "PURE.md, PROJECT.md",
  });
  assertDeepEqual(instructionsInput({ ...draft, projectDocMaxBytes: "bad", fallbackFilenames: " PURE.md, , AGENTS.md " }), {
    baseOverride: "base",
    developer: "dev",
    user: "user",
    projectDocMaxBytes: 65536,
    projectDocFallbackFilenames: ["PURE.md", "AGENTS.md"],
  });
}

function settingsMcpNormalizeCleansEmptyRowsAndPreservesMetadata() {
  const server: McpServerRecord = {
    id: " built-in ",
    enabled: true,
    transport: "stdio",
    command: " node ",
    args: [" index.js ", ""],
    env: [{ key: " API_KEY ", value: "secret" }, { key: "", value: "drop" }],
    cwd: " C:/repo ",
    url: null,
    bearerTokenEnvVar: null,
    headers: [{ key: " X-Test ", value: "yes" }, { key: "", value: "" }],
    endpoint: "node",
    sourceKind: "builtIn",
    sourceLabel: "Built-in",
    sourceDetail: "detail",
    statusKind: "enabled",
    statusMessage: "ok",
    mutationPolicy: "lockedIdentity",
    availabilityKind: "available",
    availabilityMessage: "ready",
    lastCheckedAt: 1,
    toolCount: 2,
  };

  assertDeepEqual(normalizeMcpServerInput(server), {
    id: "built-in",
    enabled: true,
    transport: "stdio",
    command: "node",
    args: ["index.js"],
    env: [{ key: "API_KEY", value: "secret" }],
    cwd: "C:/repo",
    url: null,
    bearerTokenEnvVar: null,
    headers: [{ key: "X-Test", value: "yes" }],
    sourceKind: "builtIn",
    sourceLabel: "Built-in",
    sourceDetail: "detail",
    statusKind: "enabled",
    statusMessage: "ok",
    mutationPolicy: "lockedIdentity",
  });
}

function runtimeUsage(model = "model-a", tokens = 1200): RuntimeUsage {
  return {
    model,
    contextWindow: 8000,
    latestContextTokens: tokens,
    promptTokens: tokens,
    completionTokens: 300,
    cachedPromptTokens: 100,
    totalTokens: tokens + 300,
    cacheHitRate: 0.1,
    estimatedCosts: [{ currency: "USD", amount: 0.02 }],
    hasUnpricedUsage: false,
    updatedAt: 10,
  };
}

function agentSnapshot(id: string, runtime: RuntimeUsage | null): AgentDto {
  return {
    id,
    sessionId: "session-1",
    path: id,
    parentPath: null,
    role: "reviewer",
    task: "Review code",
    status: "running",
    summary: null,
    depth: 1,
    error: null,
    reason: null,
    budgetLimitKind: null,
    budgetUsage: null,
    runtimeUsage: runtime,
    updatedAt: 10,
  };
}

function sessionRecord(
  id: string,
  updatedAt: number,
  visibility: SessionRecord["visibility"] = "active",
  parentSessionId: string | null = null,
): SessionRecord {
  return {
    id,
    projectId: "project-1",
    title: id,
    mode: "auto",
    updatedAt,
    visibility,
    parentSessionId,
  };
}

function toolPart(id: string, messageId: string, order: number, name: string, args: Record<string, unknown>): StudioPart {
  return {
    ...part(id, messageId, "turn-1", order, "tool", ""),
    tool: {
      toolCallId: id,
      name,
      arguments: JSON.stringify(args),
      result: null,
      exitCode: null,
      timedOut: false,
      workingDirectory: null,
      denialReason: null,
    },
  };
}

function storeTimelineKeys(store: MessageStore, sessionId: string, activeUserMessageId: string, statusBusy = false) {
  return constructTimelineRows({
    messages: store.messages[sessionId] ?? [],
    getMessageParts: (id) => store.parts[id] ?? [],
    getPartDelta: (partId) => store.partTextAccumDelta[partId],
    showReasoning: true,
    statusBusy,
    activeUserMessageId,
  }).map(timelineRowKey);
}

function findStorePart(store: MessageStore, messageId: string, partId: string) {
  return (store.parts[messageId] ?? []).find((item) => item.partId === partId);
}

function reasoningRowsUsePartIdentity() {
  const messages = [
    message("turn-1:user", "turn-1", "user", 1),
    message("turn-1:assistant", "turn-1", "assistant", 2),
  ];
  const partsByMessage = new Map<string, StudioPart[]>([
    ["turn-1:user", [part("turn-1:user-text", "turn-1:user", "turn-1", 1, "text", "prompt")]],
    [
      "turn-1:assistant",
      [
        part("turn-1:reason-a", "turn-1:assistant", "turn-1", 2, "reasoning", "checking a"),
        part("turn-1:reason-b", "turn-1:assistant", "turn-1", 3, "reasoning", "checking b"),
      ],
    ],
  ]);
  const rows = constructTimelineRows({
    messages,
    getMessageParts: (id) => partsByMessage.get(id) ?? [],
    showReasoning: true,
    statusBusy: true,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(rows.map((row) => row.tag), ["UserMessage", "AssistantPart", "AssistantPart", "BottomSpacer"]);
  assertDeepEqual(rows.map(timelineRowKey), [
    "user-message:turn-1:user",
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1:reason-a",
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1:reason-b",
    "bottom-spacer",
  ]);
}

function thinkingPlaceholderOnlyWhenNoAssistantParts() {
  const messages = [message("turn-1:user", "turn-1", "user", 1)];
  const rows = constructTimelineRows({
    messages,
    getMessageParts: () => [],
    showReasoning: true,
    statusBusy: true,
    activeUserMessageId: "turn-1:user",
  });
  assertDeepEqual(rows.map((row) => row.tag), ["UserMessage", "Thinking", "BottomSpacer"]);
}

function contextToolsGroupByFirstPart() {
  const parts = [
    toolPart("read-a", "turn-1:assistant", 1, "read_file", { path: "a.ts" }),
    toolPart("search-b", "turn-1:assistant", 2, "search_files", { path: "b.ts" }),
    toolPart("bash-c", "turn-1:assistant", 3, "bash", { command: "cargo test" }),
  ];
  const groups = groupParts(parts.map((item) => ({ messageID: item.messageId, part: item })));
  assertEqual(groups.length, 2);
  assertEqual(groups[0]?.key, "context:read-a");
  assertEqual(groups[0]?.type, "context");
  assertEqual(groups[1]?.key, "part:turn-1:assistant:bash-c");
}

function questionToolFollowsOpencodeTimelineVisibility() {
  const messages = [
    message("turn-1:user", "turn-1", "user", 1),
    message("turn-1:assistant", "turn-1", "assistant", 2),
  ];
  const user = part("turn-1:user-text", "turn-1:user", "turn-1", 1, "text", "prompt");
  const question = toolPart("question-a", "turn-1:assistant", 2, "request_user_input", {
    questions: [
      {
        id: "choice",
        header: "Choice",
        question: "Pick one",
        options: [{ label: "A", description: "First" }],
      },
    ],
  });
  question.status = "running";
  const partsByMessage = new Map<string, StudioPart[]>([
    ["turn-1:user", [user]],
    ["turn-1:assistant", [question]],
  ]);

  const pendingRows = constructTimelineRows({
    messages,
    getMessageParts: (id) => partsByMessage.get(id) ?? [],
    showReasoning: true,
    statusBusy: true,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(pendingRows.map(timelineRowKey), [
    "user-message:turn-1:user",
    "thinking:turn-1:user",
    "bottom-spacer",
  ]);

  question.status = "completed";
  question.tool!.result = JSON.stringify({ answers: { choice: { answers: ["A"] } } });
  const completedRows = constructTimelineRows({
    messages,
    getMessageParts: (id) => partsByMessage.get(id) ?? [],
    showReasoning: true,
    statusBusy: false,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(completedRows.map(timelineRowKey), [
    "user-message:turn-1:user",
    "assistant-part:turn-1:user:part:turn-1:assistant:question-a",
    "bottom-spacer",
  ]);
}

function liveDeltaMutatesOnlyTargetPart() {
  const store = emptyStore();
  const assistant = message("turn-1:assistant", "turn-1", "assistant", 1);
  const reasonA = part("reason-a", assistant.messageId, "turn-1", 1, "reasoning", "");
  const reasonB = part("reason-b", assistant.messageId, "turn-1", 2, "reasoning", "");
  applyStudioEventBatch(store, [
    event(1, { type: "messageUpdated", message: assistant }),
    event(2, { type: "messagePartUpdated", part: reasonA }),
    event(3, { type: "messagePartUpdated", part: reasonB }),
    event(0, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: reasonB.partId,
        field: "reasoningText",
        delta: "new thought",
      },
    }),
  ], "session-1");

  const parts = store.parts[assistant.messageId] ?? [];
  assertEqual(parts.find((item) => item.partId === reasonA.partId)?.text, "");
  assertEqual(parts.find((item) => item.partId === reasonB.partId)?.text, "");
  assertEqual(store.partTextAccumDelta[reasonB.partId], "new thought");
}

function toolBoundaryRowsKeepNewOutputAfterTool() {
  const messages = [
    message("turn-1:user", "turn-1", "user", 1),
    message("turn-1:assistant", "turn-1", "assistant", 2),
  ];
  const partsByMessage = new Map<string, StudioPart[]>([
    ["turn-1:user", [part("turn-1:user-text", "turn-1:user", "turn-1", 1, "text", "prompt")]],
    [
      "turn-1:assistant",
      [
        part("turn-1-inf-0-reasoning-1", "turn-1:assistant", "turn-1", 2, "reasoning", "before"),
        toolPart("turn-1-call_1", "turn-1:assistant", 3, "bash", { command: "pwd" }),
        part("turn-1-inf-1-reasoning-1", "turn-1:assistant", "turn-1", 4, "reasoning", "after"),
        part("turn-1-inf-1-text-final-1", "turn-1:assistant", "turn-1", 5, "text", "done"),
      ],
    ],
  ]);
  const rows = constructTimelineRows({
    messages,
    getMessageParts: (id) => partsByMessage.get(id) ?? [],
    showReasoning: true,
    statusBusy: false,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(rows.map(timelineRowKey), [
    "user-message:turn-1:user",
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1-inf-0-reasoning-1",
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1-call_1",
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1-inf-1-reasoning-1",
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1-inf-1-text-final-1",
    "bottom-spacer",
  ]);
}

function liveDeltaCreatesRowsWithoutMutatingSnapshot() {
  const messages = [
    message("turn-1:user", "turn-1", "user", 1),
    message("turn-1:assistant", "turn-1", "assistant", 2),
  ];
  const textPart = part("text-a", "turn-1:assistant", "turn-1", 2, "text", "");
  const planPart = {
    ...part("plan-a", "turn-1:assistant", "turn-1", 3, "plan", ""),
    plan: { content: "" },
  };
  const tool = toolPart("tool-a", "turn-1:assistant", 4, "bash", { command: "pwd" });
  tool.tool!.result = "";
  const partsByMessage = new Map<string, StudioPart[]>([
    ["turn-1:user", [part("turn-1:user-text", "turn-1:user", "turn-1", 1, "text", "prompt")]],
    ["turn-1:assistant", [textPart, planPart, tool]],
  ]);
  const delta = new Map([
    [textPart.partId, "streamed text"],
    [planPart.partId, "streamed plan"],
    [tool.partId, "streamed result"],
  ]);
  const rows = constructTimelineRows({
    messages,
    getMessageParts: (id) => partsByMessage.get(id) ?? [],
    getPartDelta: (partId) => delta.get(partId),
    showReasoning: true,
    statusBusy: true,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(rows.map(timelineRowKey), [
    "user-message:turn-1:user",
    "assistant-part:turn-1:user:part:turn-1:assistant:text-a",
    "assistant-part:turn-1:user:part:turn-1:assistant:plan-a",
    "assistant-part:turn-1:user:part:turn-1:assistant:tool-a",
    "bottom-spacer",
  ]);
  assertEqual(textPart.text, "");
  assertEqual(planPart.plan.content, "");
  assertEqual(tool.tool?.result, "");
  assertEqual(readPartText(delta.get(textPart.partId), textPart), "streamed text");
  assertEqual(readPlanContent(delta.get(planPart.partId), planPart), "streamed plan");
  assertEqual(readToolArguments(delta.get(tool.partId), tool), tool.tool?.arguments);
  assertEqual(readToolResult(delta.get(tool.partId), tool), "streamed result");
}

function livePlanMarkdownUsesStreamSafeBlocks() {
  const blocks = streamMarkdown("## Plan\n\n```ts\nconst value = 1", true);
  assertEqual(blocks.length, 2);
  assertEqual(blocks[0]?.mode, "full");
  assertEqual(blocks[1]?.src, "```ts\nconst value = 1");
  assertDeepEqual(streamMarkdown("## Plan\n\n- done\n- typing", true), [
    {
      raw: "## Plan\n\n",
      src: "## Plan\n\n",
      mode: "full",
    },
    {
      raw: "- done\n- typing",
      src: "- done\n- typing",
      mode: "live",
    },
  ]);
  assertDeepEqual(streamMarkdown("## Plan\n\n- done", false), [{
    raw: "## Plan\n\n- done",
    src: "## Plan\n\n- done",
    mode: "full",
  }]);
}

function reasoningDefaultCollapsedDataMarker() {
  const reason = {
    ...part("reason-a", "turn-1:assistant", "turn-1", 1, "reasoning", "## Heading\n\nbody"),
    status: "completed" as const,
  };

  assertEqual(reasoningDefaultOpen(reason), false);
  assertEqual(reasoningDefaultOpen(reason, false), false);
  assertEqual(reasoningDefaultOpen(reason, true), true);
}

function runningReasoningShowsHeadingOnly() {
  const reason = part("reason-a", "turn-1:assistant", "turn-1", 1, "reasoning", "## Inspecting files\n\nprivate detail");

  assertEqual(reasoningDefaultOpen(reason, true), false);
  assertEqual(reasoningHeading(reason.text), "Inspecting files");
}

function timelineRowReuseKeepsStableObjects() {
  const previous = constructTimelineRows({
    messages: [
      message("turn-1:user", "turn-1", "user", 1),
      message("turn-1:assistant", "turn-1", "assistant", 2),
    ],
    getMessageParts: (id) => id === "turn-1:assistant"
      ? [part("text-a", "turn-1:assistant", "turn-1", 1, "text", "hello")]
      : [part("user-a", "turn-1:user", "turn-1", 0, "text", "prompt")],
    showReasoning: true,
    statusBusy: false,
    activeUserMessageId: "turn-1:user",
  });
  const next = constructTimelineRows({
    messages: [
      message("turn-1:user", "turn-1", "user", 1),
      message("turn-1:assistant", "turn-1", "assistant", 2),
    ],
    getMessageParts: (id) => id === "turn-1:assistant"
      ? [part("text-a", "turn-1:assistant", "turn-1", 1, "text", "hello")]
      : [part("user-a", "turn-1:user", "turn-1", 0, "text", "prompt")],
    showReasoning: true,
    statusBusy: false,
    activeUserMessageId: "turn-1:user",
  });
  const reused = reuseTimelineRows(previous, next);

  assertEqual(reused[0] === previous[0], true);
  assertEqual(reused[1] === previous[1], true);
  assertEqual(reused[2] === previous[2], true);
}

function thinkingHeadingAndErrorUnwrap() {
  const messages = [
    message("turn-1:user", "turn-1", "user", 1),
    {
      ...message("turn-1:assistant", "turn-1", "assistant", 2),
      error: 'Error: {"error":{"type":"invalid_request","message":"bad input"}}',
    },
  ];
  const reason = part("reason-a", "turn-1:assistant", "turn-1", 1, "reasoning", "");
  const rows = constructTimelineRows({
    messages,
    getMessageParts: (id) => id === "turn-1:assistant" ? [reason] : [],
    getPartDelta: (partId) => partId === reason.partId ? "**Planning next**" : undefined,
    showReasoning: false,
    statusBusy: true,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(rows.map((row) => row.tag), ["UserMessage", "Error", "Thinking", "BottomSpacer"]);
  assertEqual((rows[1] as Extract<(typeof rows)[number], { tag: "Error" }>).text, "invalid_request: bad input");
  assertEqual((rows[2] as Extract<(typeof rows)[number], { tag: "Thinking" }>).reasoningHeading, "Planning next");
}

function terminalSnapshotClearsDeltaAndWinsBatch() {
  const store = emptyStore();
  const assistant = message("turn-1:assistant", "turn-1", "assistant", 1);
  const start = part("text-a", assistant.messageId, "turn-1", 1, "text", "");
  const terminal = { ...start, text: "terminal", status: "completed" as const };
  applyStudioEventBatch(store, [
    event(1, { type: "messageUpdated", message: assistant }),
    event(2, { type: "messagePartUpdated", part: start }),
    event(0, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: start.partId,
        field: "text",
        delta: "stale",
      },
    }),
    event(3, { type: "messagePartUpdated", part: terminal }),
  ], "session-1", new Set(["session-1:turn-1:assistant:text-a"]));

  assertEqual(store.parts[assistant.messageId]?.[0]?.text, "terminal");
  assertEqual(store.partTextAccumDelta[start.partId], undefined);
}

function staleDeltaAfterCoalescedSnapshotIsSkipped() {
  const store = emptyStore();
  const assistant = message("turn-1:assistant", "turn-1", "assistant", 1);
  const terminal = { ...part("text-a", assistant.messageId, "turn-1", 1, "text", "terminal"), status: "completed" as const };
  applyStudioEventBatch(store, [
    event(1, { type: "messageUpdated", message: assistant }),
    event(3, { type: "messagePartUpdated", part: terminal }),
    event(2, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: terminal.partId,
        field: "text",
        delta: " stale",
      },
    }),
  ], "session-1", new Set(["session-1:turn-1:assistant:text-a"]));

  assertEqual(store.parts[assistant.messageId]?.[0]?.text, "terminal");
}

function oldDeltaCannotPolluteTerminalSnapshotAcrossFlushes() {
  const store = emptyStore();
  const assistant = message("turn-1:assistant", "turn-1", "assistant", 1);
  const terminal = { ...part("text-a", assistant.messageId, "turn-1", 1, "text", "terminal"), status: "completed" as const };
  applyStudioEventBatch(store, [
    event(1, { type: "messageUpdated", message: assistant }),
    event(3, { type: "messagePartUpdated", part: terminal }),
  ], "session-1");
  applyStudioEventBatch(store, [
    event(3, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: terminal.partId,
        field: "text",
        delta: " stale",
      },
    }),
  ], "session-1");

  assertEqual(store.parts[assistant.messageId]?.[0]?.text, "terminal");
}

function reasoningChunksAccumulateByChunkIndex() {
  const store = emptyStore();
  const assistant = message("turn-1:assistant", "turn-1", "assistant", 1);
  const reason = part("reason-a", assistant.messageId, "turn-1", 1, "reasoning", "");
  applyStudioEventBatch(store, [
    event(1, { type: "messageUpdated", message: assistant }),
    event(2, { type: "messagePartUpdated", part: reason }),
    event(3, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: reason.partId,
        field: "reasoningText",
        delta: "later",
        chunkIndex: 1,
      },
    }),
    event(4, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: reason.partId,
        field: "reasoningText",
        delta: "first ",
        chunkIndex: 0,
      },
    }),
    event(5, {
      type: "messagePartDelta",
      delta: {
        sessionId: "session-1",
        messageId: assistant.messageId,
        partId: reason.partId,
        field: "reasoningText",
        delta: " chunk",
        chunkIndex: 0,
      },
    }),
  ], "session-1");

  assertEqual(store.parts[assistant.messageId]?.[0]?.text, "");
  assertEqual(store.partTextAccumDelta[reason.partId], "first  chunklater");
}

function realUserSnapshotRemovesOptimisticUser() {
  const store = emptyStore();
  const optimisticMessage = message("optimistic-message-1", "optimistic-turn-1", "user", 1);
  const optimisticPart = {
    ...part("optimistic-user-1", optimisticMessage.messageId, optimisticMessage.turnId, 1, "text", "hello"),
    textChannel: "user" as const,
  };
  const realMessage = message("turn-1:user", "turn-1", "user", 2);
  const realPart = {
    ...part("turn-1:user-text", realMessage.messageId, realMessage.turnId, 2, "text", "hello"),
    textChannel: "user" as const,
  };
  applyStudioEventBatch(store, [
    event(1, { type: "messageUpdated", message: optimisticMessage }),
    event(2, { type: "messagePartUpdated", part: optimisticPart }),
    event(3, { type: "messageUpdated", message: realMessage }),
    event(4, { type: "messagePartUpdated", part: realPart }),
  ], "session-1");

  assertDeepEqual((store.messages["session-1"] ?? []).map((item) => item.messageId), ["turn-1:user"]);
  assertEqual(store.parts[optimisticMessage.messageId], undefined);
}

function activeInteractionUsesKindPriority() {
  const store = emptyStore();
  applyStudioEventBatch(store, [
    event(1, {
      type: "interactionChanged",
      event: { interaction: interaction("plan", "planConfirmation", 1) },
    }),
    event(2, {
      type: "interactionChanged",
      event: { interaction: interaction("input", "userInput", 2) },
    }),
  ], "session-1");

  assertEqual(store.activeInteractionId, "input");
  assertEqual(store.activeInteractionPhase, "userInput");

  applyStudioEventBatch(store, [
    event(3, {
      type: "interactionChanged",
      event: { interaction: interaction("approval", "toolApproval", 3) },
    }),
  ], "session-1");

  assertEqual(store.activeInteractionId, "approval");
  assertEqual(store.activeInteractionPhase, "toolApproval");
}

function interactionComposerDataCoversAllPendingKinds() {
  assertEqual(interactionTitle(interaction("input", "userInput", 1)), "Input required");
  assertEqual(interactionTitle(interaction("approval", "toolApproval", 1)), "Tool approval");
  assertEqual(interactionTitle(interaction("plan", "planConfirmation", 1)), "Plan confirmation");
  assertEqual(activeInteractionPhase(interaction("input", "userInput", 1)), "userInput");
  assertEqual(activeInteractionPhase(interaction("approval", "toolApproval", 1)), "toolApproval");
  assertEqual(activeInteractionPhase(interaction("plan", "planConfirmation", 1)), "planConfirmation");
  assertEqual(prettyJson({ command: "pwd" }), "{\n  \"command\": \"pwd\"\n}");
  assertEqual(prettyJson("{\"command\":\"pwd\"}"), "{\n  \"command\": \"pwd\"\n}");
  assertEqual(prettyJson("raw args"), "raw args");
}

function interactionResolutionPayloadsMatchPureSchema() {
  const questions: UserQuestion[] = [
    {
      id: "scope",
      header: "Scope",
      question: "Pick scope",
      isOther: true,
      isSecret: false,
      options: [
        { label: "Tests", description: "Add tests" },
        { label: "Docs", description: "Update docs" },
      ],
    },
    {
      id: "token",
      header: "Token",
      question: "Provide token",
      isOther: false,
      isSecret: true,
      options: null,
    },
  ];

  assertDeepEqual(buildUserInputResolution(questions, {
    scope: { selected: ["Tests"], freeText: "Custom note" },
    token: { freeText: "secret-value" },
  }), {
    type: "userInput",
    answers: {
      scope: { answers: ["Tests", "Custom note"] },
      token: { answers: ["secret-value"] },
    },
  });

  assertDeepEqual(buildToolApprovalResolution("approved", " looks safe "), {
    type: "toolApproval",
    decision: "approved",
    reason: "looks safe",
  });
  assertDeepEqual(buildToolApprovalResolution("denied", ""), {
    type: "toolApproval",
    decision: "denied",
    reason: null,
  });
  assertDeepEqual(buildPlanConfirmationResolution("implementFreshContext", "", ""), {
    type: "planConfirmation",
    decision: "implementFreshContext",
  });
  assertDeepEqual(buildPlanConfirmationResolution("continuePlanning", "revise the plan", ""), {
    type: "planConfirmation",
    decision: "continuePlanning",
    content: "revise the plan",
    reason: "continue planning",
  });
  assertDeepEqual(buildPlanConfirmationResolution("dismiss", "", "not needed"), {
    type: "planConfirmation",
    decision: "dismiss",
    reason: "not needed",
  });
}

function resolvedAndCancelledInteractionsRestoreOrdinaryComposer() {
  const store = emptyStore();
  applyStudioEventBatch(store, [
    event(1, {
      type: "interactionChanged",
      event: { interaction: interaction("input", "userInput", 1) },
    }),
  ], "session-1");
  assertEqual(store.activeInteractionId, "input");

  applyStudioEventBatch(store, [
    event(2, {
      type: "interactionChanged",
      event: {
        interaction: {
          ...interaction("input", "userInput", 1),
          status: "resolved",
          resolvedAt: 2,
          resolution: { type: "userInput", answers: { choice: { answers: ["A"] } } },
        },
      },
    }),
  ], "session-1");
  assertEqual(store.activeInteractionId, null);

  applyStudioEventBatch(store, [
    event(3, {
      type: "interactionChanged",
      event: { interaction: interaction("approval", "toolApproval", 3) },
    }),
    event(4, {
      type: "interactionChanged",
      event: {
        interaction: {
          ...interaction("approval", "toolApproval", 3),
          status: "cancelled",
          resolvedAt: 4,
          resolution: { type: "toolApproval", decision: "denied", reason: "cancelled" },
        },
      },
    }),
  ], "session-1");
  assertEqual(store.activeInteractionId, null);
}

function busyStateBelongsToSelectedSessionOnly() {
  const store = emptyStore();
  applyStudioEventBatch(store, [{
    eventId: "event-background",
    sessionId: "session-2",
    sequence: 1,
    createdAt: 1,
    kind: {
      type: "turnChanged",
      turn: {
        turnId: "turn-background",
        sessionId: "session-2",
        status: "streaming",
        updatedAt: 1,
      },
    },
  }], "session-1");

  assertEqual(store.busy, false);
  assertEqual(store.sessionBusy["session-2"], true);

  applyStudioEventBatch(store, [event(2, {
    type: "turnChanged",
    turn: {
      turnId: "turn-current",
      sessionId: "session-1",
      status: "waitingForInteraction",
      updatedAt: 2,
    },
  })], "session-1");

  assertEqual(store.busy, true);
  assertEqual(store.turnPhase["session-1"], "approval");

  applyStudioEventBatch(store, [event(3, {
    type: "turnChanged",
    turn: {
      turnId: "turn-current",
      sessionId: "session-1",
      status: "cancelled",
      updatedAt: 3,
    },
  })], "session-1");

  assertEqual(store.busy, false);
  assertEqual(store.sessionBusy["session-1"], false);
  assertEqual(store.sessionBusy["session-2"], true);
}

function selectedSessionViewOwnsRuntimeHealthAndAgents() {
  const store = emptyStore();
  store.mcpServers = [
    {
      id: "global-mcp",
      enabled: true,
      transport: "stdio",
      command: "mcp",
      args: [],
      env: [],
      headers: [],
      endpoint: "mcp",
      sourceKind: "user",
      sourceLabel: "user",
      statusKind: "enabled",
      mutationPolicy: "userEditable",
      availabilityKind: "available",
    },
  ];
  store.lspServers = [{
    id: "rust",
    displayName: "Rust",
    extensions: ["rs"],
    languageIds: ["rust"],
    availabilityKind: "available",
    diagnosticCount: 0,
    activityKind: "idle",
  }];
  store.sessionRuntime["session-1"] = {
    sessionId: "session-1",
    usage: runtimeUsage("planner", 2000),
    activeSkills: ["rust"],
    activeMcpServers: ["global-mcp"],
    activeLspServers: ["rust"],
    updatedAt: 10,
  };
  store.sessionRuntime["session-2"] = {
    sessionId: "session-2",
    usage: runtimeUsage("other", 4000),
    activeSkills: ["docs"],
    activeMcpServers: [],
    activeLspServers: [],
    updatedAt: 20,
  };
  store.agents["session-1"] = [agentSnapshot("agent-1", runtimeUsage("executor", 600))];

  const view = selectedSessionView(store);
  assertEqual(view.runtime?.usage.model, "planner");
  assertDeepEqual(view.activeMcpServers, ["global-mcp"]);
  assertDeepEqual(view.activeLspServers, ["rust"]);
  assertEqual(view.agents[0]?.runtimeUsage?.model, "executor");
}

function sessionRuntimeEventDoesNotPolluteSelectedRuntimeHealth() {
  const store = emptyStore();
  store.sessionRuntime["session-1"] = {
    sessionId: "session-1",
    usage: runtimeUsage("current", 1000),
    activeSkills: [],
    activeMcpServers: ["current-mcp"],
    activeLspServers: ["current-lsp"],
    updatedAt: 1,
  };
  store.activeMcpServers = ["current-mcp"];
  store.activeLspServers = ["current-lsp"];

  applyStudioEventBatch(store, [{
    eventId: "event-runtime-background",
    sessionId: "session-2",
    sequence: 4,
    createdAt: 4,
    kind: {
      type: "sessionRuntimeChanged",
      runtime: {
        sessionId: "session-2",
        usage: runtimeUsage("background", 5000),
        activeSkills: [],
        activeMcpServers: ["background-mcp"],
        activeLspServers: ["background-lsp"],
        updatedAt: 4,
      },
    },
  }], "session-1");

  const view = selectedSessionView(store);
  assertDeepEqual(view.activeMcpServers, ["current-mcp"]);
  assertDeepEqual(view.activeLspServers, ["current-lsp"]);
  assertEqual(store.sessionRuntime["session-2"]?.usage.model, "background");
}

function agentSnapshotPreservesRuntimeUsageWhenUpdateOmitsIt() {
  const store = emptyStore();
  applyStudioEventBatch(store, [event(1, {
    type: "agentChanged",
    agent: agentSnapshot("agent-1", runtimeUsage("executor", 700)),
  })], "session-1");
  applyStudioEventBatch(store, [event(2, {
    type: "agentChanged",
    agent: {
      ...agentSnapshot("agent-1", null),
      status: "waiting",
      runtimeUsage: null,
      updatedAt: 20,
    },
  })], "session-1");

  const agent = store.agents["session-1"]?.[0];
  assertEqual(agent?.status, "waiting");
  assertEqual(agent?.runtimeUsage?.model, "executor");
  assertEqual(agent?.runtimeUsage?.latestContextTokens, 700);
}

function visibleProjectSessionsAreDedupedSortedAndActiveOnly() {
  const store = emptyStore();
  store.selectedProjectId = "project-1";
  store.sessions = [
    sessionRecord("older", 1),
    sessionRecord("hidden", 5, "handoffOrigin"),
    sessionRecord("implementation-child", 25, "active", "older"),
    sessionRecord("newer", 10),
    { ...sessionRecord("older", 20), title: "older latest" },
    { ...sessionRecord("other-project", 30), projectId: "project-2" },
  ];

  const visible = visibleProjectSessions(store);
  assertDeepEqual(visible.map((session) => `${session.id}:${session.title}`), [
    "older:older latest",
    "newer:newer",
  ]);
}

function visibleProjectSessionsIgnoreClosedProjectAndArchivedRows() {
  const store = emptyStore();
  store.selectedProjectId = "project-2";
  store.sessions = [
    { ...sessionRecord("old-project", 30), projectId: "project-1" },
    { ...sessionRecord("archived-current", 40, "archived"), projectId: "project-2" },
    { ...sessionRecord("child-current", 50, "active", "parent"), projectId: "project-2" },
    { ...sessionRecord("root-current", 60), projectId: "project-2" },
  ];

  assertDeepEqual(visibleProjectSessions(store).map((session) => session.id), ["root-current"]);
}

function handoffTargetSessionIsKeptButDoesNotSwitchSelectedSession() {
  const store = emptyStore();
  store.selectedProjectId = "project-1";
  store.selectedSessionId = "origin";
  store.sessions = [sessionRecord("origin", 1)];
  const target = {
    ...sessionRecord("implementation", 2, "active", "origin"),
    title: "实施计划",
  };

  applyStudioEventBatch(store, [event(1, {
    type: "sessionHandoffChanged",
    handoff: {
      originSessionId: "origin",
      targetSessionId: target.id,
      targetSession: target,
      kind: "planImplementation",
      status: "running",
      planId: "plan-1",
      updatedAt: 2,
    },
  })], "origin");

  assertEqual(store.selectedSessionId, "origin");
  assertEqual(store.sessions.some((session) => session.id === target.id), true);
  assertDeepEqual(visibleProjectSessions(store).map((session) => session.id), ["origin"]);
}

function ignoredSyntheticUserAnchorStillShowsAssistantParts() {
  const messages = [
    message("turn-1:user", "turn-1", "user", 1),
    message("turn-1:assistant", "turn-1", "assistant", 2),
  ];
  const hiddenUser = {
    ...part("turn-1:user-text", "turn-1:user", "turn-1", 1, "text", "实施计划"),
    synthetic: true,
    ignored: true,
  };
  const assistant = part("turn-1:text", "turn-1:assistant", "turn-1", 2, "text", "done");
  const partsByMessage = new Map<string, StudioPart[]>([
    ["turn-1:user", [hiddenUser]],
    ["turn-1:assistant", [assistant]],
  ]);
  const rows = constructTimelineRows({
    messages,
    getMessageParts: (id) => partsByMessage.get(id) ?? [],
    showReasoning: true,
    statusBusy: false,
    activeUserMessageId: "turn-1:user",
  });

  assertDeepEqual(rows.map(timelineRowKey), [
    "assistant-part:turn-1:user:part:turn-1:assistant:turn-1:text",
    "bottom-spacer",
  ]);
}

function statusbarWaitingPhaseUsesActiveInteractionPriority() {
  assertEqual(activeWaitingPhase(interaction("plan", "planConfirmation", 1)), "planConfirmation");
  assertEqual(activeWaitingPhase(interaction("input", "userInput", 1)), "userInput");
  assertEqual(activeWaitingPhase(interaction("approval", "toolApproval", 1)), "toolApproval");
  assertEqual(activeWaitingPhase({
    ...interaction("done", "toolApproval", 1),
    status: "resolved",
  }), null);

  const store = emptyStore();
  applyStudioEventBatch(store, [
    event(1, { type: "interactionChanged", event: { interaction: interaction("plan", "planConfirmation", 1) } }),
    event(2, { type: "interactionChanged", event: { interaction: interaction("input", "userInput", 2) } }),
    event(3, { type: "interactionChanged", event: { interaction: interaction("approval", "toolApproval", 3) } }),
  ], "session-1");

  assertEqual(activeWaitingPhase(store.interactions[store.activeInteractionId ?? ""]), "toolApproval");
}

function userInputInteractionGoldenResolvesOptionsOtherAndSecret() {
  const fixture = opencodeTimelineFixtures.userInput;
  const questions = fixture.interaction.payload.type === "userInput" ? fixture.interaction.payload.questions : [];

  assertDeepEqual(buildUserInputResolution(questions, fixture.draft), fixture.expectedResolution);
}

function reasoningAndCommentaryGoldenUseLiveOverlayUntilTerminalSnapshot() {
  const fixture = opencodeTimelineFixtures.reasoningCommentaryStaged;
  const store = emptyStore();
  applyStudioEventBatch(store, fixture.liveEvents, opencodeTimelineFixtures.sessionId);

  const reasoning = findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.reasoningPartId);
  const commentary = findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.commentaryPartId);
  assertEqual(reasoning?.text, "");
  assertEqual(commentary?.text, "");
  assertEqual(readPartText(store.partTextAccumDelta[fixture.reasoningPartId], reasoning ?? { text: "" }), fixture.expectedLiveReasoning);
  assertEqual(readPartText(store.partTextAccumDelta[fixture.commentaryPartId], commentary ?? { text: "" }), fixture.expectedLiveCommentary);
  assertDeepEqual(
    storeTimelineKeys(store, opencodeTimelineFixtures.sessionId, opencodeTimelineFixtures.userMessageId, true),
    fixture.expectedRowKeys,
  );

  applyStudioEventBatch(store, fixture.terminalEvents, opencodeTimelineFixtures.sessionId);

  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.reasoningPartId)?.text, fixture.expectedTerminalReasoning);
  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.commentaryPartId)?.text, fixture.expectedTerminalCommentary);
  assertEqual(store.partTextAccumDelta[fixture.reasoningPartId], undefined);
  assertEqual(store.partTextAccumDelta[fixture.commentaryPartId], undefined);
  assertDeepEqual(
    storeTimelineKeys(store, opencodeTimelineFixtures.sessionId, opencodeTimelineFixtures.userMessageId),
    fixture.expectedRowKeys,
  );
}

function toolApprovalGoldenKeepsToolStatusAndAllowsContinuationParts() {
  const fixture = opencodeTimelineFixtures.toolApprovalContinuation;
  const store = emptyStore();
  applyStudioEventBatch(store, fixture.startEvents, opencodeTimelineFixtures.sessionId);

  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.toolPartId)?.status, "awaitingApproval");
  assertEqual(store.activeInteractionId, "interaction-tool-approval-1");
  assertEqual(store.activeInteractionPhase, "toolApproval");

  applyStudioEventBatch(store, fixture.approvalEvents, opencodeTimelineFixtures.sessionId);

  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.toolPartId)?.status, "approved");
  assertEqual(store.activeInteractionId, null);
  assertEqual(store.activeInteractionPhase, null);

  applyStudioEventBatch(store, fixture.continuationEvents, opencodeTimelineFixtures.sessionId);

  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.toolPartId)?.status, "completed");
  assertDeepEqual(
    storeTimelineKeys(store, opencodeTimelineFixtures.sessionId, opencodeTimelineFixtures.userMessageId),
    fixture.expectedRowKeys,
  );
}

function reloadBackfillGoldenConvergesFromDurableSnapshotsOnly() {
  const fixture = opencodeTimelineFixtures.reloadBackfill;
  const liveStore = emptyStore();
  applyStudioEventBatch(liveStore, fixture.liveEvents, opencodeTimelineFixtures.sessionId);
  assertEqual(liveStore.partTextAccumDelta[fixture.reasoningPartId], "Live reasoning overlay.");
  assertEqual(liveStore.partTextAccumDelta[fixture.finalPartId], "Live final overlay.");

  const reloadedStore = emptyStore();
  applyStudioProjection(
    reloadedStore,
    opencodeTimelineFixtures.sessionId,
    fixture.messageSnapshots,
    fixture.partSnapshots,
    fixture.nextSequence,
  );

  assertEqual(reloadedStore.partTextAccumDelta[fixture.reasoningPartId], undefined);
  assertEqual(reloadedStore.partTextAccumDelta[fixture.finalPartId], undefined);
  assertEqual(findStorePart(reloadedStore, opencodeTimelineFixtures.assistantMessageId, fixture.reasoningPartId)?.text, fixture.expectedTerminalReasoning);
  assertEqual(findStorePart(reloadedStore, opencodeTimelineFixtures.assistantMessageId, fixture.finalPartId)?.text, fixture.expectedTerminalFinal);
  assertDeepEqual(
    storeTimelineKeys(reloadedStore, opencodeTimelineFixtures.sessionId, opencodeTimelineFixtures.userMessageId),
    fixture.expectedRowKeys,
  );
}

function reasoningGoldenDropsOrphansKeepsPartsSeparateAndSnapshotWins() {
  const fixture = opencodeTimelineFixtures.reasoningEdgeCases;
  const store = emptyStore();
  applyStudioEventBatch(store, [...fixture.baseEvents, ...fixture.edgeEvents], opencodeTimelineFixtures.sessionId);

  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.reasonAId)?.text, fixture.expectedReasonAText);
  assertEqual(findStorePart(store, opencodeTimelineFixtures.assistantMessageId, fixture.reasonBId)?.text, "");
  assertEqual(store.partTextAccumDelta[fixture.reasonAId], undefined);
  assertEqual(store.partTextAccumDelta[fixture.reasonBId], fixture.expectedReasonBOverlay);
  assertEqual(store.partTextAccumDelta[fixture.orphanPartId], undefined);
  assertDeepEqual(
    storeTimelineKeys(store, opencodeTimelineFixtures.sessionId, opencodeTimelineFixtures.userMessageId),
    fixture.expectedRowKeys,
  );
}

reasoningRowsUsePartIdentity();
thinkingPlaceholderOnlyWhenNoAssistantParts();
contextToolsGroupByFirstPart();
questionToolFollowsOpencodeTimelineVisibility();
liveDeltaMutatesOnlyTargetPart();
toolBoundaryRowsKeepNewOutputAfterTool();
liveDeltaCreatesRowsWithoutMutatingSnapshot();
livePlanMarkdownUsesStreamSafeBlocks();
reasoningDefaultCollapsedDataMarker();
runningReasoningShowsHeadingOnly();
timelineRowReuseKeepsStableObjects();
thinkingHeadingAndErrorUnwrap();
terminalSnapshotClearsDeltaAndWinsBatch();
staleDeltaAfterCoalescedSnapshotIsSkipped();
oldDeltaCannotPolluteTerminalSnapshotAcrossFlushes();
reasoningChunksAccumulateByChunkIndex();
realUserSnapshotRemovesOptimisticUser();
activeInteractionUsesKindPriority();
interactionComposerDataCoversAllPendingKinds();
interactionResolutionPayloadsMatchPureSchema();
resolvedAndCancelledInteractionsRestoreOrdinaryComposer();
busyStateBelongsToSelectedSessionOnly();
selectedSessionViewOwnsRuntimeHealthAndAgents();
sessionRuntimeEventDoesNotPolluteSelectedRuntimeHealth();
agentSnapshotPreservesRuntimeUsageWhenUpdateOmitsIt();
visibleProjectSessionsAreDedupedSortedAndActiveOnly();
visibleProjectSessionsIgnoreClosedProjectAndArchivedRows();
handoffTargetSessionIsKeptButDoesNotSwitchSelectedSession();
ignoredSyntheticUserAnchorStillShowsAssistantParts();
statusbarWaitingPhaseUsesActiveInteractionPriority();
userInputInteractionGoldenResolvesOptionsOtherAndSecret();
reasoningAndCommentaryGoldenUseLiveOverlayUntilTerminalSnapshot();
toolApprovalGoldenKeepsToolStatusAndAllowsContinuationParts();
reloadBackfillGoldenConvergesFromDurableSnapshotsOnly();
reasoningGoldenDropsOrphansKeepsPartsSeparateAndSnapshotWins();
settingsRolesNormalizeAfterProviderOrModelRemoval();
settingsInstructionsRoundTripDraftAndInput();
settingsMcpNormalizeCleansEmptyRowsAndPreservesMetadata();
