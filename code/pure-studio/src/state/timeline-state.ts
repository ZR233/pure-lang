import type {
  AgentEvent,
  TimelineEventRecord,
  TimelineItem,
  TimelineItemDeltaEvent,
  TimelineTracePayload,
} from "../types";

export type TimelineStateSlice = {
  timelineEvents: Map<number, TimelineEventRecord>;
  timelineItems: Map<string, TimelineItem>;
  timelineOrder: string[];
  timelineNextSequence: number;
};

export function emptyTimelineState(): TimelineStateSlice {
  return {
    timelineEvents: new Map(),
    timelineItems: new Map(),
    timelineOrder: [],
    timelineNextSequence: 0,
  };
}

export function resetTimeline<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string | null,
  events: TimelineEventRecord[],
  nextSequence: number,
): T {
  return replayTimelineEvents({
    ...state,
    ...emptyTimelineState(),
    timelineNextSequence: nextSequence,
  }, events, nextSequence);
}

export function mergeTimelineSnapshot<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string | null,
  events: TimelineEventRecord[],
  nextSequence: number,
): T {
  if (nextSequence < state.timelineNextSequence) {
    return applyTimelineEvents(state, events, nextSequence, "missingOnly");
  }
  return resetTimeline(state, _sessionId, events, nextSequence);
}

export function mergeRunPromptTimeline<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string,
  events: TimelineEventRecord[],
  timelineNextSequence: number,
): T {
  return applyTimelineEvents(state, events, timelineNextSequence, "upsert");
}

export function applyLiveTimelineEvent<T extends TimelineStateSlice>(
  state: T,
  sessionId: string,
  event: AgentEvent,
): T {
  const record = timelineRecordFromAgentEvent(sessionId, event);
  return record ? applyTimelineEventRecord(state, record, "upsert") : state;
}

export function removeOptimisticTimelineItems<T extends TimelineStateSlice>(state: T): T {
  return removeOptimisticTimelineItemsMatching(state, (itemId) => itemId.startsWith("optimistic-"));
}

export function removeOptimisticUserTimelineItems<T extends TimelineStateSlice>(state: T): T {
  return removeOptimisticTimelineItemsMatching(state, (itemId) => itemId.startsWith("optimistic-user-"));
}

export function removeOptimisticWaitingTimelineItems<T extends TimelineStateSlice>(state: T): T {
  return removeOptimisticTimelineItemsMatching(state, (itemId) => itemId.startsWith("optimistic-waiting-"));
}

function removeOptimisticTimelineItemsMatching<T extends TimelineStateSlice>(
  state: T,
  shouldRemove: (itemId: string) => boolean,
): T {
  const timelineItems = new Map(state.timelineItems);
  for (const itemId of state.timelineOrder) {
    if (shouldRemove(itemId)) {
      timelineItems.delete(itemId);
    }
  }
  return {
    ...state,
    timelineItems,
    timelineOrder: state.timelineOrder.filter((itemId) => !shouldRemove(itemId)),
  };
}

export function applyTimelineRecords<T extends TimelineStateSlice>(
  state: T,
  events: TimelineEventRecord[],
  timelineNextSequence?: number,
): T {
  return applyTimelineEvents(state, events, timelineNextSequence, "upsert");
}

function replayTimelineEvents<T extends TimelineStateSlice>(
  state: T,
  events: TimelineEventRecord[],
  timelineNextSequence: number,
): T {
  let next = state;
  for (const event of events ?? []) {
    next = applyTimelineEventRecord(next, event, "upsert");
  }
  return {
    ...next,
    timelineNextSequence: Math.max(next.timelineNextSequence, timelineNextSequence),
  };
}

function applyTimelineEvents<T extends TimelineStateSlice>(
  state: T,
  events: TimelineEventRecord[],
  timelineNextSequence: number | undefined,
  mode: "upsert" | "missingOnly",
): T {
  let next = state;
  for (const event of events ?? []) {
    next = applyTimelineEventRecord(next, event, mode);
  }
  return {
    ...next,
    timelineNextSequence:
      timelineNextSequence === undefined
        ? next.timelineNextSequence
        : Math.max(next.timelineNextSequence, timelineNextSequence),
  };
}

function applyTimelineEventRecord<T extends TimelineStateSlice>(
  state: T,
  record: TimelineEventRecord,
  mode: "upsert" | "missingOnly",
): T {
  if (!record || typeof record.sequence !== "number") {
    return state;
  }
  if (state.timelineEvents.has(record.sequence)) {
    return state;
  }
  const timelineEvents = new Map(state.timelineEvents);
  timelineEvents.set(record.sequence, record);
  return applyTracePayload({ ...state, timelineEvents }, {
    sequence: record.sequence,
    createdAt: record.createdAt,
    payload: record.payload,
  });
}

function applyTracePayload<T extends TimelineStateSlice>(
  state: T,
  record: {
    sequence: number;
    createdAt: number;
    payload: TimelineTracePayload;
  },
): T {
  switch (record.payload.type) {
    case "timelineItemStarted":
      return upsertTimelineItem(state, record.payload.item, record.sequence, "started");
    case "timelineItemDelta":
      return applyTimelineDelta(state, record.payload.event, record.sequence);
    case "timelineItemCompleted":
      return upsertTimelineItem(state, record.payload.item, record.sequence, "terminal");
    case "timelineItemFailed": {
      const item = {
        ...record.payload.item,
        content:
          record.payload.item.content?.trim() ||
          record.payload.error ||
          record.payload.item.content,
      };
      return upsertTimelineItem(state, item, record.sequence, "terminal");
    }
    case "planLifecycleChanged":
    case "interactionChanged":
    case "skillActivated":
    case "enabledToolsRecorded":
      return {
        ...state,
        timelineNextSequence: Math.max(state.timelineNextSequence, record.sequence + 1),
      };
  }
}

type TimelineItemMergeMode = "started" | "delta" | "terminal";

function upsertTimelineItem<T extends TimelineStateSlice>(
  state: T,
  item: TimelineItem,
  eventSequence?: number,
  mergeMode: TimelineItemMergeMode = "terminal",
): T {
  if (!item?.itemId) {
    return state;
  }
  const timelineItems = new Map(state.timelineItems);
  const existing = timelineItems.get(item.itemId);
  timelineItems.set(item.itemId, mergeTimelineItem(existing, item, mergeMode));
  const timelineOrder = (existing ? [...state.timelineOrder] : [...state.timelineOrder, item.itemId])
    .sort((left, right) => compareTimelineItemOrder(left, right, timelineItems));
  return {
    ...state,
    timelineItems,
    timelineOrder,
    timelineNextSequence:
      eventSequence === undefined
        ? state.timelineNextSequence
        : Math.max(state.timelineNextSequence, eventSequence + 1),
  };
}

function compareTimelineItemOrder(
  left: string,
  right: string,
  timelineItems: Map<string, TimelineItem>,
): number {
  const leftItem = timelineItems.get(left);
  const rightItem = timelineItems.get(right);
  const order = (leftItem?.sequence ?? 0) - (rightItem?.sequence ?? 0);
  if (order !== 0) {
    return order;
  }
  const leftWaiting = left.startsWith("optimistic-waiting-");
  const rightWaiting = right.startsWith("optimistic-waiting-");
  if (leftWaiting !== rightWaiting) {
    return leftWaiting ? 1 : -1;
  }
  return left.localeCompare(right);
}

function mergeTimelineItem(
  existing: TimelineItem | undefined,
  item: TimelineItem,
  mergeMode: TimelineItemMergeMode,
): TimelineItem {
  const incoming = normalizeTimelineItem(item);
  if (!existing) {
    return incoming;
  }
  const current = normalizeTimelineItem(existing);
  if (mergeMode === "delta") {
    return {
      ...incoming,
      sequence: Math.min(current.sequence, incoming.sequence),
      createdAt: Math.min(current.createdAt, incoming.createdAt),
    };
  }
  if (mergeMode === "started") {
    return {
      ...current,
      ...incoming,
      status: current.status,
      updatedAt: Math.max(current.updatedAt, incoming.updatedAt),
      content: mergeTimelineContent(current.content, incoming.content, mergeMode),
      thinkingChunks:
        current.thinkingChunks.length > 0 ? current.thinkingChunks : incoming.thinkingChunks,
      tool: mergeTimelineTool(current.tool, incoming.tool),
      agent: current.agent ?? incoming.agent ?? null,
      inference: current.inference ?? incoming.inference ?? null,
      usage: current.usage ?? incoming.usage ?? null,
      sequence: Math.min(current.sequence, incoming.sequence),
      createdAt: Math.min(current.createdAt, incoming.createdAt),
    };
  }
  return {
    ...current,
    ...incoming,
    content: mergeTimelineContent(current.content, incoming.content, mergeMode),
    thinkingChunks:
      incoming.thinkingChunks.length > 0 ? incoming.thinkingChunks : current.thinkingChunks,
    tool: mergeTimelineTool(current.tool, incoming.tool),
    agent: incoming.agent ?? current.agent ?? null,
    inference: incoming.inference ?? current.inference ?? null,
    usage: incoming.usage ?? current.usage ?? null,
    sequence: Math.min(current.sequence, incoming.sequence),
    createdAt: Math.min(current.createdAt, incoming.createdAt),
  };
}

function mergeTimelineContent(
  current: string,
  incoming: string,
  mergeMode: TimelineItemMergeMode,
): string {
  switch (mergeMode) {
    case "started":
      return current || incoming || "";
    case "delta":
      return incoming;
    case "terminal":
      return incoming || current || "";
  }
}

function mergeTimelineTool(
  current: TimelineItem["tool"] | null | undefined,
  incoming: TimelineItem["tool"] | null | undefined,
): TimelineItem["tool"] | null {
  if (!current && !incoming) {
    return null;
  }
  if (!current) {
    return incoming ? { ...incoming } : null;
  }
  if (!incoming) {
    return { ...current };
  }
  return {
    ...current,
    ...incoming,
    name: incoming.name || current.name,
    arguments: incoming.arguments || current.arguments || "",
    result: incoming.result ?? current.result ?? null,
    exitCode: incoming.exitCode ?? current.exitCode ?? null,
    timedOut: incoming.timedOut || current.timedOut || false,
    workingDirectory: incoming.workingDirectory ?? current.workingDirectory ?? null,
    denialReason: incoming.denialReason ?? current.denialReason ?? null,
  };
}

function applyTimelineDelta<T extends TimelineStateSlice>(
  state: T,
  event: TimelineItemDeltaEvent,
  eventSequence: number,
): T {
  const existing = state.timelineItems.get(event.itemId) ?? blankTimelineItem(event);
  const item = normalizeTimelineItem(existing);
  item.status = event.status;
  item.updatedAt = event.updatedAt;
  const delta = event.delta;
  switch (delta.type) {
    case "text":
      item.textChannel = delta.textChannel;
      item.content += delta.delta;
      break;
    case "plan":
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
      item.tool = item.tool ?? blankTimelineToolItem(item.itemId);
      item.tool.arguments += delta.delta;
      break;
    case "toolResult":
      item.tool = item.tool ?? blankTimelineToolItem(item.itemId);
      item.tool.result = `${item.tool.result ?? ""}${delta.delta}`;
      break;
  }
  return upsertTimelineItem(state, item, eventSequence, "delta");
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
    textChannel: event.delta.type === "text" ? event.delta.textChannel : null,
    content: "",
    attachments: [],
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
    attachments: (item.attachments ?? []).map((attachment) => ({ ...attachment })),
    thinkingChunks: (item.thinkingChunks ?? []).map((chunk) => ({ ...chunk })),
    tool: item.tool ? { ...item.tool } : null,
    agent: item.agent ? { ...item.agent } : null,
    inference: item.inference ? { ...item.inference } : null,
    usage: item.usage ? { ...item.usage } : null,
  };
}

function blankTimelineToolItem(itemId: string): NonNullable<TimelineItem["tool"]> {
  return {
    toolCallId: itemId,
    name: "",
    arguments: "",
    result: null,
    exitCode: null,
    timedOut: false,
    workingDirectory: null,
    denialReason: null,
  };
}

function timelineRecordFromAgentEvent(
  sessionId: string,
  event: AgentEvent,
): TimelineEventRecord | null {
  if (!sessionId || event === "done") {
    return null;
  }
  if ("timelineItemStarted" in event) {
    const item = event.timelineItemStarted.item;
    return {
      id: `live-${sessionId}-${item.sequence}`,
      sessionId,
      sequence: item.sequence,
      createdAt: item.createdAt,
      kind: "TimelineItemStarted",
      payload: { type: "timelineItemStarted", item },
    };
  }
  if ("timelineItemDelta" in event) {
    const deltaEvent = event.timelineItemDelta.event;
    return {
      id: `live-${sessionId}-${deltaEvent.sequence}`,
      sessionId,
      sequence: deltaEvent.sequence,
      createdAt: deltaEvent.createdAt,
      kind: "TimelineItemDelta",
      payload: { type: "timelineItemDelta", event: deltaEvent },
    };
  }
  if ("timelineItemCompleted" in event) {
    return {
      id: `live-${sessionId}-${event.timelineItemCompleted.sequence}`,
      sessionId,
      sequence: event.timelineItemCompleted.sequence,
      createdAt: event.timelineItemCompleted.item.updatedAt,
      kind: "TimelineItemCompleted",
      payload: { type: "timelineItemCompleted", item: event.timelineItemCompleted.item },
    };
  }
  if ("timelineItemFailed" in event) {
    return {
      id: `live-${sessionId}-${event.timelineItemFailed.sequence}`,
      sessionId,
      sequence: event.timelineItemFailed.sequence,
      createdAt: event.timelineItemFailed.item.updatedAt,
      kind: "TimelineItemFailed",
      payload: {
        type: "timelineItemFailed",
        item: event.timelineItemFailed.item,
        error: event.timelineItemFailed.error,
      },
    };
  }
  return null;
}
