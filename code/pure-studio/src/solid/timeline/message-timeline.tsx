// SPDX-License-Identifier: MIT
// Adapted from opencode packages/app/src/pages/session/message-timeline.tsx.

import { For, Match, Show, Switch, createEffect, createMemo, createSignal, on, onCleanup } from "solid-js";
import { Virtualizer, type VirtualizerHandle } from "virtua/solid";
import { ArrowDownToLine, Loader2 } from "lucide-solid";
import type { StudioMessage, StudioPart } from "../../types";
import i18n from "../../i18n";
import { ContextToolGroup, MessagePart, partDefaultOpen } from "./message-part";
import {
  constructTimelineRows,
  reuseTimelineRows,
  sameTimelineKeys,
  timelineRowKey,
  type TimelineRow,
} from "./message-timeline.data";
import { MarkdownContent } from "../markdown";

const timelineCacheLimit = 16;
const timelineFallbackItemSize = 72;
const bottomAnchorSettleFrames = 6;
const timelineCache = new Map<string, { keys: readonly string[]; cache: VirtualizerHandle["cache"] }>();

export function MessageTimeline(props: {
  sessionId: string | null;
  messages: StudioMessage[];
  getMessageParts: (messageId: string) => StudioPart[];
  getPart: (messageId: string, partId: string) => StudioPart | undefined;
  getPartDelta: (partId: string) => string | undefined;
  busy: boolean;
  empty?: string;
}) {
  let root: HTMLDivElement | undefined;
  let virtualizer: VirtualizerHandle | undefined;
  let virtualizerSessionKey = props.sessionId ?? "none";
  let virtualizerRowKeys: readonly string[] = [];
  let bottomAnchorSessionKey = "";
  let bottomAnchorFrame: number | undefined;
  let bottomAnchorFrames = 0;
  let measuredBottomAnchored = true;
  const [scrollJump, setScrollJump] = createSignal(false);
  const [userPinnedBottom, setUserPinnedBottom] = createSignal(true);
  const activeUserMessageId = createMemo(() => {
    for (let index = props.messages.length - 1; index >= 0; index -= 1) {
      const message = props.messages[index];
      if (message?.role === "user") return message.messageId;
    }
  });
  const rows = createMemo((previous: TimelineRow[] | undefined) => {
    const next = constructTimelineRows({
      messages: props.messages,
      getMessageParts: props.getMessageParts,
      getPartDelta: props.getPartDelta,
      showReasoning: true,
      statusBusy: props.busy,
      activeUserMessageId: activeUserMessageId(),
    });
    return reuseTimelineRows(previous, next);
  });
  const rowKeys = createMemo(() => rows().map(timelineRowKey), [] as string[], { equals: sameTimelineKeys });
  const sessionKey = () => props.sessionId ?? "none";
  const virtualCache = createMemo(() => readTimelineCache(sessionKey(), rowKeys()));
  const keepMounted = createMemo(() => {
    const id = activeUserMessageId();
    if (!id) return undefined;
    const currentRows = rows();
    for (let index = currentRows.length - 1; index >= 0; index -= 1) {
      const row = currentRows[index];
      if (row && "userMessageID" in row && row.userMessageID === id) return [index];
    }
    return undefined;
  });
  const activeAssistantContentVersion = createMemo(() => {
    const activeTurnId = props.messages.find((message) => message.messageId === activeUserMessageId())?.turnId;
    if (!activeTurnId) return "";
    return props.messages
      .filter((message) => message.role === "assistant" && message.turnId === activeTurnId)
      .flatMap((message) => [
        `${message.messageId}:${message.status}:${message.completedAt ?? ""}:${message.error ?? ""}`,
        ...props.getMessageParts(message.messageId).map((part) => {
          const delta = props.getPartDelta(part.partId);
          if (part.partType === "text" || part.partType === "reasoning" || part.partType === "plan") {
            return `${part.partId}:${part.partType}:${part.status}:${(delta ?? part.plan?.content ?? part.text ?? "").length}`;
          }
          if (part.partType === "tool") {
            return `${part.partId}:tool:${part.status}:${(part.tool?.arguments ?? "").length}:${(delta ?? part.tool?.result ?? "").length}`;
          }
          return `${part.partId}:${part.partType}:${part.status}`;
        }),
      ])
      .join("|");
  });

  createEffect(
    on(
      () => [rowKeys(), activeAssistantContentVersion(), props.busy] as const,
      () => {
        if (!virtualizer || (!userPinnedBottom() && !measuredBottomAnchored)) return;
        const keys = rowKeys();
        if (keys.length === 0) return;
        virtualizer.scrollToIndex(keys.length - 1, { align: "end" });
        scheduleMeasuredBottomAnchor();
      },
      { defer: true },
    ),
  );

  createEffect(
    on(
      () => [sessionKey(), rowKeys()] as const,
      ([nextSessionKey, nextRowKeys]) => {
        virtualizerSessionKey = nextSessionKey;
        virtualizerRowKeys = nextRowKeys;
        maybeAnchorBottom();
      },
      { defer: true },
    ),
  );

  createEffect(
    on(
      sessionKey,
      () => {
        setUserPinnedBottom(true);
        setScrollJump(false);
        measuredBottomAnchored = true;
        bottomAnchorSessionKey = "";
        requestAnimationFrame(() => scrollToBottom());
      },
    ),
  );

  onCleanup(() => {
    if (bottomAnchorFrame !== undefined) cancelAnimationFrame(bottomAnchorFrame);
    writeTimelineCache(virtualizerSessionKey, virtualizerRowKeys, virtualizer);
  });

  function bindRoot(el: HTMLDivElement) {
    root = el;
    measuredBottomAnchored = isMeasuredBottom(el);
  }

  function onScroll() {
    if (!root) return;
    measuredBottomAnchored = isMeasuredBottom(root);
    const atBottom = root.scrollHeight - root.scrollTop - root.clientHeight < 48;
    setUserPinnedBottom(atBottom);
    setScrollJump(!atBottom);
  }

  function scrollToBottom() {
    if (!virtualizer) return;
    const keys = rowKeys();
    if (keys.length === 0) return;
    virtualizer.scrollToIndex(keys.length - 1, { align: "end" });
    measuredBottomAnchored = true;
    setScrollJump(false);
    setUserPinnedBottom(true);
    scheduleMeasuredBottomAnchor();
  }

  function maybeAnchorBottom() {
    const key = sessionKey();
    if (bottomAnchorSessionKey === key) return;
    if (!virtualizer) return;
    const keys = rowKeys();
    if (keys.length === 0) return;
    bottomAnchorSessionKey = key;
    if (!userPinnedBottom()) return;
    virtualizer.scrollToIndex(keys.length - 1, { align: "end" });
    scheduleMeasuredBottomAnchor();
  }

  function scheduleMeasuredBottomAnchor() {
    bottomAnchorFrames = bottomAnchorSettleFrames;
    if (bottomAnchorFrame !== undefined) return;
    const tick = () => {
      bottomAnchorFrame = undefined;
      if (bottomAnchorFrames === bottomAnchorSettleFrames || bottomAnchorFrames === 1) {
        measureTimeline();
      }
      if (!anchorMeasuredBottom()) {
        bottomAnchorFrames = 0;
        return;
      }
      bottomAnchorFrames -= 1;
      if (bottomAnchorFrames <= 0) return;
      bottomAnchorFrame = requestAnimationFrame(tick);
    };
    bottomAnchorFrame = requestAnimationFrame(tick);
  }

  function measureTimeline() {
    (virtualizer as (VirtualizerHandle & { measure?: () => void }) | undefined)?.measure?.();
  }

  function anchorMeasuredBottom() {
    if (!root) return false;
    if (!measuredBottomAnchored && !userPinnedBottom()) return false;
    root.scrollTop = root.scrollHeight;
    measuredBottomAnchored = true;
    setScrollJump(false);
    return true;
  }

  function isMeasuredBottom(el: HTMLDivElement) {
    return el.scrollHeight - el.clientHeight - el.scrollTop <= 4;
  }

  return (
    <div class="oc-timeline-root">
      <div class="oc-jump" data-show={scrollJump() || undefined}>
        <button type="button" onClick={scrollToBottom} aria-label={i18n.t("toolCall.scrollToBottom")}>
          <ArrowDownToLine size={16} />
        </button>
      </div>
      <div class="oc-scroll" ref={bindRoot} onScroll={onScroll}>
        <Show when={rows().length > 0} fallback={<div class="oc-empty">{props.empty ?? i18n.t("conversation.emptyTitle")}</div>}>
          <Virtualizer
            data={rows()}
            cache={virtualCache()}
            itemSize={virtualCache() ? undefined : timelineFallbackItemSize}
            keepMounted={keepMounted()}
            scrollRef={root}
            startMargin={48}
            ref={(handle: VirtualizerHandle | undefined) => {
              if (!handle) {
                writeTimelineCache(virtualizerSessionKey, virtualizerRowKeys, virtualizer);
                virtualizer = undefined;
                return;
              }
              virtualizer = handle;
              virtualizerSessionKey = sessionKey();
              virtualizerRowKeys = rowKeys();
              maybeAnchorBottom();
            }}
          >
            {(row: TimelineRow) => (
              <TimelineRowView
                row={row}
                getPart={props.getPart}
                getMessageParts={props.getMessageParts}
                getPartDelta={props.getPartDelta}
              />
            )}
          </Virtualizer>
        </Show>
      </div>
    </div>
  );
}

function TimelineRowView(props: {
  row: TimelineRow;
  getMessageParts: (messageId: string) => StudioPart[];
  getPart: (messageId: string, partId: string) => StudioPart | undefined;
  getPartDelta: (partId: string) => string | undefined;
}) {
  return (
    <Switch>
      <Match when={props.row.tag === "UserMessage"}>
        <UserMessageRow row={props.row as Extract<TimelineRow, { tag: "UserMessage" }>} getMessageParts={props.getMessageParts} />
      </Match>
      <Match when={props.row.tag === "AssistantPart"}>
        <AssistantPartRow
          row={props.row as Extract<TimelineRow, { tag: "AssistantPart" }>}
          getPart={props.getPart}
          getPartDelta={props.getPartDelta}
        />
      </Match>
      <Match when={props.row.tag === "Thinking"}>
        <div class="oc-row">
          <div class="oc-assistant-frame oc-thinking-row">
            <Loader2 size={14} class="spin" />
            <span>{(props.row as Extract<TimelineRow, { tag: "Thinking" }>).reasoningHeading ?? i18n.t("timeline.thinkingActive")}</span>
          </div>
        </div>
      </Match>
      <Match when={props.row.tag === "Error"}>
        <div class="oc-row">
          <div class="oc-error">{(props.row as Extract<TimelineRow, { tag: "Error" }>).text}</div>
        </div>
      </Match>
      <Match when={props.row.tag === "BottomSpacer"}>
        <div class="oc-bottom-spacer" />
      </Match>
    </Switch>
  );
}

function UserMessageRow(props: {
  row: Extract<TimelineRow, { tag: "UserMessage" }>;
  getMessageParts: (messageId: string) => StudioPart[];
}) {
  const parts = () => props.getMessageParts(props.row.userMessageID);
  const text = () => parts().map((part) => part.text).join("\n").trim();
  return (
    <div class="oc-row oc-user-row">
      <div class="oc-user-bubble">
        <Show when={text()}>
          {(value) => <MarkdownContent content={value()} />}
        </Show>
        <For each={parts().flatMap((part) => part.attachments ?? [])}>
          {(attachment) => (
            <div class="oc-attachment">
              <Show when={attachment.dataUrl}>
                <img src={attachment.dataUrl ?? ""} alt={attachment.filename ?? attachment.mediaType} />
              </Show>
              <span>{attachment.filename ?? attachment.mediaType}</span>
            </div>
          )}
        </For>
      </div>
    </div>
  );
}

function AssistantPartRow(props: {
  row: Extract<TimelineRow, { tag: "AssistantPart" }>;
  getPart: (messageId: string, partId: string) => StudioPart | undefined;
  getPartDelta: (partId: string) => string | undefined;
}) {
  const parts = () => {
    const group = props.row.group;
    if (group.type === "part") {
      const part = props.getPart(group.ref.messageID, group.ref.partID);
      return part ? [part] : [];
    }
    return group.refs.flatMap((ref) => {
      const part = props.getPart(ref.messageID, ref.partID);
      return part ? [part] : [];
    });
  };
  return (
    <div class="oc-row">
      <div class="oc-assistant-frame" data-previous={props.row.previousAssistantPart || undefined}>
        <Show
          when={props.row.group.type === "context"}
          fallback={
            <For each={parts()}>
              {(part) => (
                <MessagePart
                  part={part}
                  deltaText={props.getPartDelta(part.partId)}
                  defaultOpen={partDefaultOpen(part, false, false)}
                />
              )}
            </For>
          }
        >
          <ContextToolGroup parts={parts()} />
        </Show>
      </div>
    </div>
  );
}

function readTimelineCache(id: string, keys: readonly string[]) {
  const entry = timelineCache.get(id);
  if (!entry) return undefined;
  if (sameTimelineKeys(entry.keys, keys)) return entry.cache;
  timelineCache.delete(id);
  return undefined;
}

function writeTimelineCache(id: string, keys: readonly string[], cache: VirtualizerHandle | undefined) {
  if (!cache || keys.length === 0) return;
  timelineCache.delete(id);
  timelineCache.set(id, { keys: keys.slice(), cache: cache.cache });
  while (timelineCache.size > timelineCacheLimit) {
    const first = timelineCache.keys().next().value;
    if (!first) break;
    timelineCache.delete(first);
  }
}
