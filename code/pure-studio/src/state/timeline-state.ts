import type { AgentEvent, TimelineItem, TimelineItemDeltaEvent } from "../types";

export type TimelineStateSlice = {
  timelineItems: Map<string, TimelineItem>;
  timelineOrder: string[];
  timelineNextSequence: number;
};

export function emptyTimelineState(): TimelineStateSlice {
  return {
    timelineItems: new Map(),
    timelineOrder: [],
    timelineNextSequence: 0,
  };
}

export function resetTimeline<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string | null,
  items: TimelineItem[],
  nextSequence: number,
): T {
  return mergeTimelineItems(
    {
      ...state,
      ...emptyTimelineState(),
      timelineNextSequence: nextSequence,
    },
    items,
  );
}

export function mergeTimelineSnapshot<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string | null,
  items: TimelineItem[],
  nextSequence: number,
): T {
  if (nextSequence < state.timelineNextSequence) {
    return mergeMissingTimelineItems(state, items);
  }
  return resetTimeline(state, _sessionId, items, nextSequence);
}

export function mergeRunPromptTimeline<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string,
  items: TimelineItem[],
  timelineNextSequence: number,
): T {
  return {
    ...mergeTimelineItems(state, items),
    timelineNextSequence: Math.max(state.timelineNextSequence, timelineNextSequence),
  };
}

export function applyLiveTimelineEvent<T extends TimelineStateSlice>(
  state: T,
  _sessionId: string,
  event: AgentEvent,
): T {
  if (event === "done") return state;
  if ("timelineItemStarted" in event) {
    const item = event.timelineItemStarted.item;
    return upsertTimelineItem(state, item, item.sequence);
  }
  if ("timelineItemDelta" in event) {
    const deltaEvent = event.timelineItemDelta.event;
    return applyTimelineDelta(state, deltaEvent, deltaEvent.sequence);
  }
  if ("timelineItemCompleted" in event) {
    return upsertTimelineItem(
      state,
      event.timelineItemCompleted.item,
      event.timelineItemCompleted.sequence,
    );
  }
  if ("timelineItemFailed" in event) {
    const item = {
      ...event.timelineItemFailed.item,
      content:
        event.timelineItemFailed.item.content?.trim() ||
        event.timelineItemFailed.error ||
        event.timelineItemFailed.item.content,
    };
    return upsertTimelineItem(
      state,
      item,
      event.timelineItemFailed.sequence,
    );
  }
  return state;
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

export function mergeTimelineItems<T extends TimelineStateSlice>(
  state: T,
  incoming: TimelineItem[],
): T {
  let next = state;
  for (const item of incoming ?? []) {
    if (!item?.itemId) continue;
    next = upsertTimelineItem(next, item);
  }
  return next;
}

function mergeMissingTimelineItems<T extends TimelineStateSlice>(
  state: T,
  incoming: TimelineItem[],
): T {
  let next = state;
  for (const item of incoming ?? []) {
    if (!item?.itemId || next.timelineItems.has(item.itemId)) continue;
    next = upsertTimelineItem(next, item);
  }
  return next;
}

function upsertTimelineItem<T extends TimelineStateSlice>(
  state: T,
  item: TimelineItem,
  eventSequence?: number,
): T {
  if (!item?.itemId) {
    return state;
  }
  const timelineItems = new Map(state.timelineItems);
  const existing = timelineItems.get(item.itemId);
  timelineItems.set(item.itemId, mergeTimelineItem(existing, item));
  const timelineOrder = existing
    ? state.timelineOrder
    : [...state.timelineOrder, item.itemId].sort((left, right) =>
        compareTimelineItemOrder(left, right, timelineItems),
      );
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

function mergeTimelineItem(existing: TimelineItem | undefined, item: TimelineItem): TimelineItem {
  const incoming = normalizeTimelineItem(item);
  if (!existing) {
    return incoming;
  }
  const current = normalizeTimelineItem(existing);
  return {
    ...current,
    ...incoming,
    content: incoming.content || current.content || "",
    thinkingChunks:
      incoming.thinkingChunks.length > 0 ? incoming.thinkingChunks : current.thinkingChunks,
    tool: mergeTimelineTool(current.tool, incoming.tool),
    agent: incoming.agent ?? current.agent ?? null,
    inference: incoming.inference ?? current.inference ?? null,
    usage: incoming.usage ?? current.usage ?? null,
    sequence: current.sequence,
    createdAt: current.createdAt,
  };
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
  return upsertTimelineItem(state, item, eventSequence);
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
    role: null,
    content: "",
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
