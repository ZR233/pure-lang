// SPDX-License-Identifier: MIT
// Adapted from opencode packages/app/src/pages/session/message-timeline.data.ts.

import type { StudioMessage, StudioPart } from "../../types";
import { groupParts, renderableWithDelta, sameGroups, type PartGroup } from "./message-part.data";
import { readPartText, reasoningHeading } from "./message-part-text";

export type TimelineRow =
  | { tag: "UserMessage"; userMessageID: string; previousUserMessage: boolean }
  | { tag: "AssistantPart"; userMessageID: string; group: PartGroup; previousAssistantPart: boolean }
  | { tag: "Thinking"; userMessageID: string; reasoningHeading?: string }
  | { tag: "Error"; userMessageID: string; text: string }
  | { tag: "BottomSpacer" };

export function timelineRowKey(row: TimelineRow) {
  switch (row.tag) {
    case "UserMessage":
      return `user-message:${row.userMessageID}`;
    case "AssistantPart":
      return `assistant-part:${row.userMessageID}:${row.group.key}`;
    case "Thinking":
      return `thinking:${row.userMessageID}`;
    case "Error":
      return `error:${row.userMessageID}`;
    case "BottomSpacer":
      return "bottom-spacer";
  }
}

export function timelineRowEquals(a: TimelineRow, b: TimelineRow) {
  if (a === b) return true;
  if (a.tag !== b.tag) return false;
  switch (a.tag) {
    case "UserMessage": {
      if (b.tag !== "UserMessage") return false;
      return a.userMessageID === b.userMessageID
        && a.previousUserMessage === b.previousUserMessage;
    }
    case "AssistantPart": {
      if (b.tag !== "AssistantPart") return false;
      return a.userMessageID === b.userMessageID
        && a.previousAssistantPart === b.previousAssistantPart
        && sameGroups([a.group], [b.group]);
    }
    case "Thinking": {
      if (b.tag !== "Thinking") return false;
      return a.userMessageID === b.userMessageID
        && a.reasoningHeading === b.reasoningHeading;
    }
    case "Error": {
      if (b.tag !== "Error") return false;
      return a.userMessageID === b.userMessageID && a.text === b.text;
    }
    case "BottomSpacer":
      return true;
  }
}

export function reuseTimelineRows(previous: readonly TimelineRow[] | undefined, rows: TimelineRow[]) {
  if (!previous?.length) return rows;
  const byKey = new Map(previous.map((row) => [timelineRowKey(row), row] as const));
  return rows.map((row) => {
    const existing = byKey.get(timelineRowKey(row));
    if (!existing) return row;
    return timelineRowEquals(existing, row) ? existing : row;
  });
}

export function constructTimelineRows(input: {
  messages: StudioMessage[];
  getMessageParts: (messageId: string) => StudioPart[];
  getPartDelta?: (partId: string) => string | undefined;
  showReasoning: boolean;
  activeUserMessageId?: string;
  statusBusy: boolean;
}) {
  const rows: TimelineRow[] = [];
  const userMessages = input.messages.filter((message) => message.role === "user");
  const assistantMessagesByTurn = new Map<string, StudioMessage[]>();
  for (const message of input.messages) {
    if (message.role !== "assistant") continue;
    const list = assistantMessagesByTurn.get(message.turnId) ?? [];
    list.push(message);
    assistantMessagesByTurn.set(message.turnId, list);
  }

  userMessages.forEach((userMessage, index) => {
    const previousUserMessage = index > 0;
    const userParts = input.getMessageParts(userMessage.messageId);
    const visibleUser = userParts.length === 0
      || userParts.some((part) => renderableWithDelta(part, input.getPartDelta?.(part.partId), true));
    if (visibleUser) rows.push({ tag: "UserMessage", userMessageID: userMessage.messageId, previousUserMessage });
    const assistantMessages = assistantMessagesByTurn.get(userMessage.turnId) ?? [];
    const assistantPartRefs = assistantMessages.flatMap((message) =>
      input.getMessageParts(message.messageId)
        .filter((part) => renderableWithDelta(part, input.getPartDelta?.(part.partId), input.showReasoning))
        .map((part) => ({ messageID: message.messageId, part })),
    );
    let assistantGroupIndex = 0;
    for (const group of groupParts(assistantPartRefs)) {
      rows.push({
        tag: "AssistantPart",
        userMessageID: userMessage.messageId,
        group,
        previousAssistantPart: assistantGroupIndex > 0,
      });
      assistantGroupIndex += 1;
    }
    const error = assistantMessages.find((message) => message.error)?.error;
    if (error) {
      rows.push({ tag: "Error", userMessageID: userMessage.messageId, text: unwrapErrorMessage(error) });
    }
    if (
      input.statusBusy &&
      input.activeUserMessageId === userMessage.messageId &&
      assistantPartRefs.length === 0
    ) {
      rows.push({
        tag: "Thinking",
        userMessageID: userMessage.messageId,
        reasoningHeading: readReasoningHeading(assistantMessages, input.getMessageParts, input.getPartDelta),
      });
    }
  });

  if (rows.length > 0) rows.push({ tag: "BottomSpacer" });
  return rows;
}

export function sameTimelineKeys(a: readonly string[] | undefined, b: readonly string[] | undefined) {
  if (a === b) return true;
  if (!a || !b) return false;
  if (a.length !== b.length) return false;
  return a.every((key, index) => key === b[index]);
}

function readReasoningHeading(
  assistantMessages: StudioMessage[],
  getMessageParts: (messageId: string) => StudioPart[],
  getPartDelta: ((partId: string) => string | undefined) | undefined,
) {
  return assistantMessages
    .flatMap((message) => getMessageParts(message.messageId))
    .map((part) => part.partType === "reasoning"
      ? reasoningHeading(readPartText(getPartDelta?.(part.partId), part))
      : undefined)
    .find((value): value is string => Boolean(value));
}

function unwrapErrorMessage(message: string) {
  const text = message.replace(/^Error:\s*/, "").trim();

  const parse = (value: string) => {
    try {
      return JSON.parse(value) as unknown;
    } catch {
      return undefined;
    }
  };

  const read = (value: string) => {
    const first = parse(value);
    if (typeof first !== "string") return first;
    return parse(first.trim());
  };

  let json = read(text);

  if (json === undefined) {
    const start = text.indexOf("{");
    const end = text.lastIndexOf("}");
    if (start !== -1 && end > start) json = read(text.slice(start, end + 1));
  }

  if (!record(json)) return message;

  const err = record(json.error) ? json.error : undefined;
  if (err) {
    const type = typeof err.type === "string" ? err.type : undefined;
    const msg = typeof err.message === "string" ? err.message : undefined;
    if (type && msg) return `${type}: ${msg}`;
    if (msg) return msg;
    if (type) return type;
    const code = typeof err.code === "string" ? err.code : undefined;
    if (code) return code;
  }

  const msg = typeof json.message === "string" ? json.message : undefined;
  if (msg) return msg;

  const reason = typeof json.error === "string" ? json.error : undefined;
  if (reason) return reason;

  return message;
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
