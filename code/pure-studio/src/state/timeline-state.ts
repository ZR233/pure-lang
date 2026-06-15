import type {
  StudioEventEnvelope,
  StudioMessage,
  StudioPart,
  StudioPartDelta,
  StudioPartDeltaField,
  TimelineItem,
} from "../types";

export type ConversationStateSlice = {
  messages: Map<string, StudioMessage>;
  partsByMessage: Map<string, StudioPart[]>;
  partDeltaAccum: Map<string, StudioPartDeltaAccum>;
  messageSequences: Map<string, number>;
  partSequences: Map<string, number>;
  eventNextSequence: number;
};

export type TimelineStateSlice = ConversationStateSlice;

export type StudioPartDeltaAccum = {
  text: string;
  reasoningText: string;
  planContent: string;
  toolArguments: string;
  toolResult: string;
  thinkingChunks: Map<number, string>;
};

export function emptyTimelineState(): TimelineStateSlice {
  return {
    messages: new Map(),
    partsByMessage: new Map(),
    partDeltaAccum: new Map(),
    messageSequences: new Map(),
    partSequences: new Map(),
    eventNextSequence: 0,
  };
}

export function applyStudioEvent<T extends TimelineStateSlice>(
  state: T,
  envelope: StudioEventEnvelope,
): T {
  switch (envelope.kind.type) {
    case "messageUpdated":
      return upsertMessage(state, envelope.kind.message, envelope.sequence);
    case "messageRemoved":
      return removeMessage(state, envelope.kind.messageId, envelope.sequence);
    case "messagePartUpdated":
      return upsertPart(state, envelope.kind.part, envelope.sequence);
    case "messagePartRemoved":
      return removePart(state, envelope.kind.messageId, envelope.kind.partId, envelope.sequence);
    case "messagePartDelta":
      return applyPartDelta(state, envelope.kind.delta);
    case "turnChanged":
    case "interactionChanged":
    case "agentChanged":
    case "agentTimelineChanged":
    case "sessionRuntimeChanged":
    case "skillActivated":
    case "planLifecycleChanged":
    case "sessionHandoffChanged":
    case "sessionListChanged":
    case "mcpHealthChanged":
    case "lspHealthChanged":
    case "stale":
      return advanceCursor(state, envelope);
  }
}

export function applyStudioEvents<T extends TimelineStateSlice>(
  state: T,
  events: StudioEventEnvelope[],
  nextSequence?: number,
): T {
  let next = state;
  for (const event of events ?? []) {
    next = applyStudioEvent(next, event);
  }
  return nextSequence === undefined
    ? next
    : { ...next, eventNextSequence: Math.max(next.eventNextSequence, nextSequence) };
}

export function resetConversation<T extends TimelineStateSlice>(
  state: T,
  events: StudioEventEnvelope[],
  nextSequence: number,
): T {
  return applyStudioEvents({ ...state, ...emptyTimelineState() }, events, nextSequence);
}

export function removeOptimisticTimelineItems<T extends TimelineStateSlice>(state: T): T {
  return removeOptimisticPartsMatching(state, (partId) => partId.startsWith("optimistic-"));
}

export function removeOptimisticUserTimelineItems<T extends TimelineStateSlice>(state: T): T {
  return removeOptimisticPartsMatching(state, (partId) => partId.startsWith("optimistic-user-"));
}

export function removeOptimisticWaitingTimelineItems<T extends TimelineStateSlice>(state: T): T {
  return removeOptimisticPartsMatching(state, (partId) => partId.startsWith("optimistic-waiting-"));
}

export function addOptimisticPart<T extends TimelineStateSlice>(
  state: T,
  message: StudioMessage,
  part: StudioPart,
): T {
  const withMessage = upsertMessage(state, message, undefined);
  return upsertPart(withMessage, part, undefined);
}

export function timelineItemWithDelta(
  item: TimelineItem,
  accum: StudioPartDeltaAccum | undefined,
): TimelineItem {
  if (!accum) return normalizeTimelineItem(item);
  const next = normalizeTimelineItem(item);
  next.content += accum.text + accum.planContent + accum.reasoningText;
  if (accum.thinkingChunks.size > 0) {
    for (const [chunkIndex, delta] of accum.thinkingChunks) {
      const chunk = next.thinkingChunks.find((part) => part.chunkIndex === chunkIndex);
      if (chunk) {
        chunk.content += delta;
      } else {
        next.thinkingChunks.push({ chunkIndex, content: delta });
      }
    }
    next.thinkingChunks.sort((left, right) => left.chunkIndex - right.chunkIndex);
  }
  if (accum.toolArguments || accum.toolResult) {
    next.tool = next.tool ?? blankTimelineToolItem(next.itemId);
    next.tool.arguments += accum.toolArguments;
    next.tool.result = `${next.tool.result ?? ""}${accum.toolResult}` || next.tool.result;
  }
  return next;
}

export function timelineItemsFromConversation(state: TimelineStateSlice): TimelineItem[] {
  const items: TimelineItem[] = [];
  const messages = [...state.messages.values()].sort(compareMessages);
  for (const message of messages) {
    const parts = (state.partsByMessage.get(message.messageId) ?? [])
      .slice()
      .sort(compareParts);
    for (const part of parts) {
      if (part.ignored) continue;
      items.push(partToTimelineItem(part));
    }
  }
  return items;
}

function upsertMessage<T extends TimelineStateSlice>(
  state: T,
  message: StudioMessage,
  sequence: number | undefined,
): T {
  const messages = new Map(state.messages);
  const existing = messages.get(message.messageId);
  if (sequence !== undefined && existing && existingSequenceIsNewer(state, message.messageId, sequence)) {
    return advanceSequence(state, sequence);
  }
  messages.set(message.messageId, normalizeMessage(message));
  const messageSequences = new Map(state.messageSequences);
  if (sequence !== undefined) {
    messageSequences.set(message.messageId, sequence);
  }
  return {
    ...state,
    messages,
    messageSequences,
    eventNextSequence: advanceSequenceValue(state.eventNextSequence, sequence),
  };
}

function removeMessage<T extends TimelineStateSlice>(
  state: T,
  messageId: string,
  sequence: number | undefined,
): T {
  const messages = new Map(state.messages);
  messages.delete(messageId);
  const messageSequences = new Map(state.messageSequences);
  messageSequences.delete(messageId);
  const partsByMessage = new Map(state.partsByMessage);
  const partDeltaAccum = new Map(state.partDeltaAccum);
  const partSequences = new Map(state.partSequences);
  for (const part of partsByMessage.get(messageId) ?? []) {
    partDeltaAccum.delete(part.partId);
    partSequences.delete(part.partId);
  }
  partsByMessage.delete(messageId);
  return {
    ...state,
    messages,
    messageSequences,
    partsByMessage,
    partDeltaAccum,
    partSequences,
    eventNextSequence: advanceSequenceValue(state.eventNextSequence, sequence),
  };
}

function upsertPart<T extends TimelineStateSlice>(
  state: T,
  part: StudioPart,
  sequence: number | undefined,
): T {
  const existingSequence = state.partSequences.get(part.partId);
  if (sequence !== undefined && existingSequence !== undefined && existingSequence > sequence) {
    return advanceSequence(state, sequence);
  }
  const partsByMessage = new Map(state.partsByMessage);
  const existingParts = partsByMessage.get(part.messageId) ?? [];
  const normalized = normalizePart(part);
  const nextParts = existingParts.some((existing) => existing.partId === part.partId)
    ? existingParts.map((existing) => existing.partId === part.partId ? normalized : existing)
    : [...existingParts, normalized];
  nextParts.sort(compareParts);
  partsByMessage.set(part.messageId, nextParts);
  const partDeltaAccum = new Map(state.partDeltaAccum);
  partDeltaAccum.delete(part.partId);
  const partSequences = new Map(state.partSequences);
  if (sequence !== undefined) {
    partSequences.set(part.partId, sequence);
  }
  return {
    ...state,
    partsByMessage,
    partDeltaAccum,
    partSequences,
    eventNextSequence: advanceSequenceValue(state.eventNextSequence, sequence),
  };
}

function removePart<T extends TimelineStateSlice>(
  state: T,
  messageId: string,
  partId: string,
  sequence: number | undefined,
): T {
  const partsByMessage = new Map(state.partsByMessage);
  partsByMessage.set(
    messageId,
    (partsByMessage.get(messageId) ?? []).filter((part) => part.partId !== partId),
  );
  const partDeltaAccum = new Map(state.partDeltaAccum);
  partDeltaAccum.delete(partId);
  const partSequences = new Map(state.partSequences);
  partSequences.delete(partId);
  return {
    ...state,
    partsByMessage,
    partDeltaAccum,
    partSequences,
    eventNextSequence: advanceSequenceValue(state.eventNextSequence, sequence),
  };
}

function applyPartDelta<T extends TimelineStateSlice>(
  state: T,
  delta: StudioPartDelta,
): T {
  if (!hasPart(state, delta.messageId, delta.partId)) {
    return state;
  }
  const partDeltaAccum = new Map(state.partDeltaAccum);
  const accum = cloneDeltaAccum(partDeltaAccum.get(delta.partId) ?? emptyDeltaAccum());
  appendDelta(accum, delta.field, delta.delta, delta.chunkIndex ?? 0);
  partDeltaAccum.set(delta.partId, accum);
  return { ...state, partDeltaAccum };
}

function removeOptimisticPartsMatching<T extends TimelineStateSlice>(
  state: T,
  shouldRemove: (partId: string) => boolean,
): T {
  const partsByMessage = new Map<string, StudioPart[]>();
  const partDeltaAccum = new Map(state.partDeltaAccum);
  const messageSequences = new Map(state.messageSequences);
  const partSequences = new Map(state.partSequences);
  const messages = new Map(state.messages);
  for (const [messageId, parts] of state.partsByMessage) {
    const nextParts = parts.filter((part) => {
      const remove = shouldRemove(part.partId);
      if (remove) {
        partDeltaAccum.delete(part.partId);
        partSequences.delete(part.partId);
      }
      return !remove;
    });
    if (nextParts.length > 0) {
      partsByMessage.set(messageId, nextParts);
    } else if (messageId.startsWith("optimistic-")) {
      messages.delete(messageId);
      messageSequences.delete(messageId);
    }
  }
  return { ...state, messages, messageSequences, partsByMessage, partDeltaAccum, partSequences };
}

function hasPart(state: TimelineStateSlice, messageId: string, partId: string): boolean {
  return (state.partsByMessage.get(messageId) ?? []).some((part) => part.partId === partId);
}

function advanceCursor<T extends TimelineStateSlice>(state: T, envelope: StudioEventEnvelope): T {
  return envelope.kind.type === "messagePartDelta"
    ? state
    : advanceSequence(state, envelope.sequence);
}

function advanceSequence<T extends TimelineStateSlice>(state: T, sequence: number | undefined): T {
  return { ...state, eventNextSequence: advanceSequenceValue(state.eventNextSequence, sequence) };
}

function advanceSequenceValue(current: number, sequence: number | undefined): number {
  return sequence === undefined ? current : Math.max(current, sequence + 1);
}

function existingSequenceIsNewer(
  state: TimelineStateSlice,
  messageId: string,
  sequence: number,
): boolean {
  const existingSequence = state.messageSequences.get(messageId);
  return existingSequence !== undefined && existingSequence > sequence;
}

function emptyDeltaAccum(): StudioPartDeltaAccum {
  return {
    text: "",
    reasoningText: "",
    planContent: "",
    toolArguments: "",
    toolResult: "",
    thinkingChunks: new Map(),
  };
}

function cloneDeltaAccum(accum: StudioPartDeltaAccum): StudioPartDeltaAccum {
  return {
    text: accum.text,
    reasoningText: accum.reasoningText,
    planContent: accum.planContent,
    toolArguments: accum.toolArguments,
    toolResult: accum.toolResult,
    thinkingChunks: new Map(accum.thinkingChunks),
  };
}

function appendDelta(
  accum: StudioPartDeltaAccum,
  field: StudioPartDeltaField,
  delta: string,
  chunkIndex: number,
) {
  switch (field) {
    case "text":
      accum.text += delta;
      break;
    case "reasoningText":
      accum.reasoningText += delta;
      accum.thinkingChunks.set(chunkIndex, `${accum.thinkingChunks.get(chunkIndex) ?? ""}${delta}`);
      break;
    case "planContent":
      accum.planContent += delta;
      break;
    case "tool.arguments":
      accum.toolArguments += delta;
      break;
    case "tool.result":
      accum.toolResult += delta;
      break;
  }
}

function partToTimelineItem(part: StudioPart): TimelineItem {
  const kind = timelineKind(part);
  const content = partContent(part);
  return {
    turnId: part.turnId,
    itemId: part.partId,
    startedSequence: part.order,
    kind,
    status: part.status,
    createdAt: part.createdAt,
    updatedAt: part.updatedAt,
    textChannel: part.textChannel ?? null,
    content,
    attachments: part.attachments ?? [],
    thinkingChunks: kind === "thinking" && content ? [{ chunkIndex: 0, content }] : [],
    tool: part.tool ?? null,
    agent: part.agent ?? null,
    inference: part.inference ?? null,
    usage: part.usage ?? null,
  };
}

function timelineKind(part: StudioPart): TimelineItem["kind"] {
  switch (part.partType) {
    case "reasoning":
      return "thinking";
    case "text":
    case "tool":
    case "agent":
    case "turn":
    case "inference":
    case "plan":
      return part.partType;
    case "file":
      return "turn";
  }
}

function partContent(part: StudioPart): string {
  if (part.partType === "plan") {
    return part.plan?.content ?? part.text ?? "";
  }
  return part.text ?? "";
}

function normalizeMessage(message: StudioMessage): StudioMessage {
  return { ...message, metadata: message.metadata ?? {} };
}

function normalizePart(part: StudioPart): StudioPart {
  return {
    ...part,
    text: part.text ?? "",
    attachments: (part.attachments ?? []).map((attachment) => ({ ...attachment })),
    tool: part.tool ? { ...part.tool } : null,
    agent: part.agent ? { ...part.agent } : null,
    inference: part.inference ? { ...part.inference } : null,
    plan: part.plan ? { ...part.plan } : null,
    file: part.file ? { ...part.file } : null,
    usage: part.usage ? { ...part.usage } : null,
    synthetic: part.synthetic ?? false,
    ignored: part.ignored ?? false,
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

function compareMessages(left: StudioMessage, right: StudioMessage): number {
  if (left.createdAt !== right.createdAt) return left.createdAt - right.createdAt;
  return left.messageId.localeCompare(right.messageId);
}

function compareParts(left: StudioPart, right: StudioPart): number {
  if (left.order !== right.order) return left.order - right.order;
  return left.partId.localeCompare(right.partId);
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
