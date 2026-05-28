import {
  Activity,
  Bot,
  Brain,
  CheckCircle2,
  ChevronDown,
  Circle,
  Clock,
  Send,
  Square,
  Terminal,
  UserRound,
  Wrench,
} from "lucide-react";
import type { TFunction } from "i18next";
import type { Dispatch, ReactNode, SetStateAction } from "react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ChatItem,
  AgentActivity,
  AgentStatus,
  ProjectRecord,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  TimelineItem,
  TrackedToolCall,
  TurnPhase,
  ToolCallStatus,
  ToolCallStatus2,
} from "../types";
import { formatTime } from "../lib/utils";
import { SessionStatusBar } from "./SessionStatusBar";

const agentStatusKeys: Record<AgentStatus, string> = {
  queued: "subagent.queued",
  running: "subagent.running",
  waiting: "subagent.awaitingTool",
  completed: "turnPhase.completed",
  errored: "subagent.failed",
  interrupted: "turnPhase.interrupted",
  shutdown: "status.done",
  notFound: "subagent.notFound",
};

const roleI18nKeys: Record<string, string> = {
  explorer: "roles.explorer",
  planner: "roles.planner",
  executor: "roles.executor",
  reviewer: "roles.reviewer",
};

type TimelineEntry =
  | {
      kind: "message";
      key: string;
      role: "system" | "user" | "assistant" | "tool";
      content: string;
    }
  | { kind: "thought"; key: string; content: string }
  | { kind: "tool"; key: string; toolCall: TrackedToolCall }
  | { kind: "agent"; key: string; activity: AgentActivity }
  | { kind: "trace"; key: string; item: TimelineItem };

type ConversationPanelProps = {
  selectedSession: SessionRecord | null;
  selectedProject: ProjectRecord | null;
  isBusy: boolean;
  chatItems: ChatItem[];
  agentActivities: AgentActivity[];
  timelineItems: TimelineItem[];
  sessionRuntime: SessionRuntime | null;
  prompt: string;
  status: string;
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
  onSetPrompt: (value: string) => void;
  onSendPrompt: () => void;
  onStopPrompt: () => void;
};

const SCROLL_THRESHOLD = 40;

function compact(value: string, max = 220): string {
  const text = value.trim();
  if (text.length <= max) {
    return text;
  }
  return `${text.slice(0, max)}...`;
}

function linkHref(href: string): string | null {
  const value = href.trim();
  if (
    value.startsWith("http://") ||
    value.startsWith("https://") ||
    value.startsWith("mailto:") ||
    value.startsWith("#")
  ) {
    return value;
  }
  return null;
}

function renderInlineMarkdown(text: string, keyPrefix: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*|\[[^\]]+\]\([^)]+\))/g;
  let cursor = 0;
  let index = 0;
  for (const match of text.matchAll(pattern)) {
    const token = match[0];
    const start = match.index ?? 0;
    if (start > cursor) {
      nodes.push(text.slice(cursor, start));
    }
    const key = `${keyPrefix}-inline-${index}`;
    if (token.startsWith("`")) {
      nodes.push(<code key={key}>{token.slice(1, -1)}</code>);
    } else if (token.startsWith("**")) {
      nodes.push(<strong key={key}>{renderInlineMarkdown(token.slice(2, -2), key)}</strong>);
    } else if (token.startsWith("*")) {
      nodes.push(<em key={key}>{renderInlineMarkdown(token.slice(1, -1), key)}</em>);
    } else {
      const link = token.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
      const href = linkHref(link?.[2] ?? "");
      nodes.push(
        href ? (
          <a key={key} href={href} target="_blank" rel="noreferrer">
            {renderInlineMarkdown(link?.[1] ?? "", key)}
          </a>
        ) : (
          token
        ),
      );
    }
    cursor = start + token.length;
    index++;
  }
  if (cursor < text.length) {
    nodes.push(text.slice(cursor));
  }
  return nodes;
}

function flushParagraph(blocks: ReactNode[], paragraph: string[], keyPrefix: string) {
  if (paragraph.length === 0) {
    return;
  }
  const text = paragraph.join("\n").trim();
  if (text) {
    blocks.push(<p key={`${keyPrefix}-p-${blocks.length}`}>{renderInlineMarkdown(text, `${keyPrefix}-p-${blocks.length}`)}</p>);
  }
  paragraph.length = 0;
}

function MarkdownContent({ content }: { content: string }) {
  const blocks: ReactNode[] = [];
  const paragraph: string[] = [];
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];
    if (line.match(/^```(\w+)?\s*$/)) {
      flushParagraph(blocks, paragraph, "md");
      const code: string[] = [];
      index++;
      while (index < lines.length && !lines[index].startsWith("```")) {
        code.push(lines[index]);
        index++;
      }
      blocks.push(
        <pre key={`md-code-${blocks.length}`} className="markdown-code">
          <code>{code.join("\n")}</code>
        </pre>,
      );
      continue;
    }
    if (!line.trim()) {
      flushParagraph(blocks, paragraph, "md");
      continue;
    }
    const heading = line.match(/^(#{1,3})\s+(.+)$/);
    if (heading) {
      flushParagraph(blocks, paragraph, "md");
      const level = heading[1].length;
      const children = renderInlineMarkdown(heading[2], `md-h-${blocks.length}`);
      blocks.push(
        level === 1 ? (
          <h2 key={`md-h-${blocks.length}`}>{children}</h2>
        ) : level === 2 ? (
          <h3 key={`md-h-${blocks.length}`}>{children}</h3>
        ) : (
          <h4 key={`md-h-${blocks.length}`}>{children}</h4>
        ),
      );
      continue;
    }
    const quote = line.match(/^>\s?(.+)$/);
    if (quote) {
      flushParagraph(blocks, paragraph, "md");
      blocks.push(
        <blockquote key={`md-quote-${blocks.length}`}>
          {renderInlineMarkdown(quote[1], `md-quote-${blocks.length}`)}
        </blockquote>,
      );
      continue;
    }
    const bullet = line.match(/^\s*[-*]\s+(.+)$/);
    if (bullet) {
      flushParagraph(blocks, paragraph, "md");
      const items = [bullet[1]];
      while (index + 1 < lines.length) {
        const next = lines[index + 1].match(/^\s*[-*]\s+(.+)$/);
        if (!next) break;
        items.push(next[1]);
        index++;
      }
      blocks.push(
        <ul key={`md-ul-${blocks.length}`}>
          {items.map((item, itemIndex) => (
            <li key={itemIndex}>{renderInlineMarkdown(item, `md-ul-${blocks.length}-${itemIndex}`)}</li>
          ))}
        </ul>,
      );
      continue;
    }
    const ordered = line.match(/^\s*\d+\.\s+(.+)$/);
    if (ordered) {
      flushParagraph(blocks, paragraph, "md");
      const items = [ordered[1]];
      while (index + 1 < lines.length) {
        const next = lines[index + 1].match(/^\s*\d+\.\s+(.+)$/);
        if (!next) break;
        items.push(next[1]);
        index++;
      }
      blocks.push(
        <ol key={`md-ol-${blocks.length}`}>
          {items.map((item, itemIndex) => (
            <li key={itemIndex}>{renderInlineMarkdown(item, `md-ol-${blocks.length}-${itemIndex}`)}</li>
          ))}
        </ol>,
      );
      continue;
    }
    paragraph.push(line);
  }
  flushParagraph(blocks, paragraph, "md");
  return <div className="markdown-content">{blocks}</div>;
}

function agentSummary(activity: AgentActivity, t: TFunction) {
  if (activity.error) {
    return activity.error;
  }
  if (activity.summary) {
    return activity.summary;
  }
  return activity.status === "queued" ? t("subagent.waitingToStart") : t("subagent.noSummaryYet");
}

function formatUsage(usage?: { promptTokens: number; completionTokens: number; totalTokens: number } | null) {
  if (!usage) return null;
  return `${usage.promptTokens}p + ${usage.completionTokens}c = ${usage.totalTokens}t`;
}

function thoughtLabel(content: string): string {
  const firstLine = content.trim().split(/\r?\n/, 1)[0]?.trim();
  return firstLine?.toLowerCase().startsWith("thought") ? firstLine : "Thought";
}

function toolStatusLabel(status: ToolCallStatus | ToolCallStatus2 | null | undefined, t: TFunction): string {
  switch (status) {
    case "streaming":
    case "started":
      return t("toolCall.streaming");
    case "completed":
      return t("toolCall.completed");
    case "pending_approval":
    case "awaiting_approval":
      return t("toolCall.pendingApproval");
    case "approved":
      return t("toolCall.approved");
    case "denied":
      return t("toolCall.denied");
    case "result_ready":
      return t("toolCall.resultReady");
    case "failed":
      return t("subagent.failed");
    default:
      return t("toolCall.streaming");
  }
}

function turnStatusLabel(status: TimelineItem["turnStatus"], t: TFunction): string {
  switch (status) {
    case "started":
      return t("turnPhase.running");
    case "completed":
      return t("turnPhase.completed");
    case "errored":
      return t("turnPhase.failed");
    case "aborted":
      return t("turnPhase.interrupted");
    default:
      return t("turnPhase.running");
  }
}

function toolDisplayName(name: string | null | undefined): string {
  return name ?? "Tool call";
}

function parseToolArguments(argumentsText: string | null | undefined): Record<string, unknown> | null {
  if (!argumentsText?.trim()) return null;
  try {
    const parsed = JSON.parse(argumentsText);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function toolPathSummary(name: string | null | undefined, argumentsText: string | null | undefined): string | null {
  const args = parseToolArguments(argumentsText);
  if (!args) return null;
  const pathValue =
    args.path ??
    args.filePath ??
    args.file_path ??
    args.targetPath ??
    args.target_path ??
    args.directory ??
    args.root;
  if (typeof pathValue === "string" && pathValue.trim()) {
    return pathValue;
  }
  const command = args.command;
  if (name === "bash" && typeof command === "string" && command.trim()) {
    return compact(command, 90);
  }
  return null;
}

function isQuietFileTool(name: string | null | undefined): boolean {
  return matchesToolName(name, ["read_file", "write_file", "list_files", "list_file"]);
}

function hidesToolResult(name: string | null | undefined): boolean {
  return matchesToolName(name, ["read_file", "write_file", "list_files", "list_file"]);
}

function matchesToolName(name: string | null | undefined, names: string[]): boolean {
  const normalized = name?.toLowerCase();
  return Boolean(normalized && names.includes(normalized));
}

function ToolDetails({
  argumentsText,
  result,
  hideResult,
  t,
}: {
  argumentsText?: string | null;
  result?: string | null;
  hideResult: boolean;
  t: TFunction;
}) {
  const hasArguments = Boolean(argumentsText?.trim());
  const hasResult = Boolean(result?.trim()) && !hideResult;
  if (!hasArguments && !hasResult) return null;
  return (
    <details className="timeline-details">
      <summary>
        <span>{t("toolCall.details")}</span>
        <ChevronDown size={14} />
      </summary>
      {hasArguments ? (
        <>
          <div className="timeline-detail-label">{t("toolCall.arguments")}</div>
          <pre className="timeline-code">{argumentsText}</pre>
        </>
      ) : null}
      {hasResult ? (
        <>
          <div className="timeline-detail-label">{t("toolCall.result")}</div>
          <pre className="timeline-code">{result}</pre>
        </>
      ) : null}
    </details>
  );
}

function timelineEntries(
  chatItems: ChatItem[],
  agentActivities: AgentActivity[],
  timelineItems: TimelineItem[],
): TimelineEntry[] {
  const entries: TimelineEntry[] = [];
  const seenToolIds = new Set<string>();

  chatItems.forEach((item, index) => {
    if (item.kind === "tool_call") {
      seenToolIds.add(item.toolCall.id);
      entries.push({ kind: "tool", key: item.key, toolCall: item.toolCall });
      return;
    }

    const { message } = item;
    const hasReasoning = Boolean(message.reasoningContent?.trim());
    const hasContent = Boolean(message.content.trim());
    if (hasReasoning) {
      entries.push({
        kind: "thought",
        key: `${item.key}-thought`,
        content: message.reasoningContent ?? "",
      });
    }
    if (hasContent || message.role !== "assistant") {
      entries.push({
        kind: "message",
        key: `${item.key}-${index}`,
        role: message.role,
        content: message.content,
      });
    }
  });

  for (const activity of agentActivities) {
    entries.push({
      kind: "agent",
      key: `agent-${activity.eventId}`,
      activity,
    });
  }

  for (const item of timelineItems) {
    if (item.kind === "tool_call" && item.toolCallId && seenToolIds.has(item.toolCallId)) {
      continue;
    }
    if (item.kind === "tool_call") {
      entries.push({ kind: "trace", key: `trace-${item.sequence}`, item });
    } else if (
      item.kind === "turn" &&
      (item.turnStatus === "errored" || item.turnStatus === "aborted")
    ) {
      entries.push({ kind: "trace", key: `trace-${item.sequence}`, item });
    }
  }

  return entries;
}

function EntryShell({
  className,
  icon,
  children,
}: {
  className: string;
  icon: ReactNode;
  children: ReactNode;
}) {
  return (
    <article className={`timeline-entry ${className}`}>
      <span className="timeline-node" aria-hidden="true">
        {icon}
      </span>
      <div className="timeline-entry-card">{children}</div>
    </article>
  );
}

function MessageEntry({ entry }: { entry: Extract<TimelineEntry, { kind: "message" }> }) {
  const roleIcon = entry.role === "user" ? <UserRound size={14} /> : <Bot size={14} />;
  return (
    <EntryShell className={`timeline-entry-message role-${entry.role}`} icon={roleIcon}>
      {entry.role === "user" ? null : (
        <div className="timeline-entry-head">
          <strong>{entry.role.toUpperCase()}</strong>
        </div>
      )}
      <div className="timeline-message-content">
        <MarkdownContent content={entry.content} />
      </div>
    </EntryShell>
  );
}

function ThoughtEntry({ content }: { content: string }) {
  return (
    <EntryShell className="timeline-entry-thought" icon={<Brain size={14} />}>
      <details className="timeline-thought">
        <summary>
          <span>{thoughtLabel(content)}</span>
          <ChevronDown size={14} />
        </summary>
        <pre>{content}</pre>
      </details>
    </EntryShell>
  );
}

function ToolEntry({ toolCall, t }: { toolCall: TrackedToolCall; t: TFunction }) {
  const pathSummary = toolPathSummary(toolCall.name, toolCall.arguments);
  const hideResult = hidesToolResult(toolCall.name);
  return (
    <EntryShell className={`timeline-entry-tool status-${toolCall.status}`} icon={<Wrench size={14} />}>
      <div className="timeline-entry-head">
        <strong>{toolCall.name}</strong>
        {pathSummary ? (
          <code className="timeline-inline-code" title={pathSummary}>
            {pathSummary}
          </code>
        ) : null}
        <span className={`timeline-badge status-${toolCall.status}`}>{toolStatusLabel(toolCall.status, t)}</span>
      </div>
      {!pathSummary && !isQuietFileTool(toolCall.name) && toolCall.arguments ? (
        <p className="timeline-result">{compact(toolCall.arguments, 160)}</p>
      ) : null}
      <ToolDetails
        argumentsText={isQuietFileTool(toolCall.name) ? null : toolCall.arguments}
        result={toolCall.result}
        hideResult={hideResult}
        t={t}
      />
    </EntryShell>
  );
}

function AgentEntry({
  activity,
  t,
}: {
  activity: AgentActivity;
  t: TFunction;
}) {
  return (
    <EntryShell className={`timeline-entry-subagent status-${activity.status}`} icon={<Activity size={14} />}>
      <div className="timeline-entry-head">
        <strong>Agent: {t(roleI18nKeys[activity.role] ?? `roles.${activity.role}`)}</strong>
        <span className={`timeline-badge status-${activity.status}`}>
          {t(agentStatusKeys[activity.status])}
        </span>
      </div>
      <div className="timeline-entry-meta">
        <span>{t("subagent.depth")} {activity.depth}</span>
        {activity.parentPath ? <span>{t("subagent.parent")} {activity.parentPath}</span> : null}
        <span>{formatTime(activity.updatedAt)}</span>
      </div>
      <details className="timeline-details">
        <summary>
          <span>{t("subagent.prompt")}</span>
          <ChevronDown size={14} />
        </summary>
        <p className="timeline-task">{activity.task}</p>
        <p className="timeline-result">{agentSummary(activity, t)}</p>
      </details>
    </EntryShell>
  );
}

function TraceEntry({ item, t }: { item: TimelineItem; t: TFunction }) {
  if (item.kind === "inference") {
    const usage = formatUsage(item.inferenceUsage);
    return (
      <EntryShell className="timeline-entry-trace status-inference" icon={<Clock size={14} />}>
        <div className="timeline-entry-head">
          <strong>{item.inferenceModel ?? "Inference"}</strong>
          {usage ? <span className="timeline-entry-meta-inline">{usage}</span> : null}
        </div>
      </EntryShell>
    );
  }

  if (item.kind === "tool_call") {
    const name = toolDisplayName(item.toolName);
    const pathSummary = toolPathSummary(item.toolName, item.toolArguments);
    const hideResult = hidesToolResult(item.toolName);
    return (
      <EntryShell className={`timeline-entry-tool status-${item.toolStatus ?? "started"}`} icon={<Wrench size={14} />}>
        <div className="timeline-entry-head">
          <strong>{name}</strong>
          {pathSummary ? (
            <code className="timeline-inline-code" title={pathSummary}>
              {pathSummary}
            </code>
          ) : null}
          <span className={`timeline-badge status-${item.toolStatus ?? "started"}`}>
            {toolStatusLabel(item.toolStatus, t)}
          </span>
        </div>
        {!pathSummary && !isQuietFileTool(item.toolName) && item.toolArguments ? (
          <p className="timeline-result">{compact(item.toolArguments, 160)}</p>
        ) : null}
        <ToolDetails
          argumentsText={isQuietFileTool(item.toolName) ? null : item.toolArguments}
          result={item.toolResult}
          hideResult={hideResult}
          t={t}
        />
      </EntryShell>
    );
  }

  const icon = item.turnStatus === "completed" ? <CheckCircle2 size={14} /> : <Circle size={14} />;
  const usage = formatUsage(item.turnUsage);
  return (
    <EntryShell className={`timeline-entry-trace status-${item.turnStatus ?? "started"}`} icon={icon}>
      <div className="timeline-entry-head">
        <strong>{t("timeline.turn")} {turnStatusLabel(item.turnStatus, t)}</strong>
        {item.turnModel ? <span className="timeline-entry-meta-inline">{item.turnModel}</span> : null}
      </div>
      {usage ? <p className="timeline-entry-meta">{usage}</p> : null}
      {item.toolResult ? <p className="timeline-result">{compact(item.toolResult, 260)}</p> : null}
    </EntryShell>
  );
}

export function ConversationPanel({
  selectedSession,
  selectedProject,
  isBusy,
  chatItems,
  agentActivities,
  timelineItems,
  sessionRuntime,
  prompt,
  status,
  turnPhase,
  turnStartedAt,
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
  onSetPrompt,
  onSendPrompt,
  onStopPrompt,
}: ConversationPanelProps) {
  const { t } = useTranslation();
  const messageStreamRef = useRef<HTMLDivElement>(null);
  const timelineRef = useRef<HTMLDivElement>(null);
  const bottomRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const followModeRef = useRef<"following" | "paused">("following");
  const programmaticScrollRef = useRef(false);
  const userInteractingRef = useRef(false);
  const userInteractingTimerRef = useRef<number | null>(null);
  const didRenderEntriesRef = useRef(false);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const prevBusyRef = useRef(isBusy);
  const entries = useMemo(
    () => timelineEntries(chatItems, agentActivities, timelineItems),
    [chatItems, agentActivities, timelineItems],
  );
  const stopping = turnPhase === "stopping";

  useEffect(() => {
    const el = messageStreamRef.current;
    if (!el) return;

    const updateBottomState = () => {
      const atBottom = isNearBottom(el);
      isAtBottomRef.current = atBottom;
      if (atBottom) {
        followModeRef.current = "following";
      }
      setShowScrollButton(followModeRef.current === "paused" || !atBottom);
      return atBottom;
    };

    const markUserInteraction = () => {
      userInteractingRef.current = true;
      if (userInteractingTimerRef.current !== null) {
        window.clearTimeout(userInteractingTimerRef.current);
      }
      userInteractingTimerRef.current = window.setTimeout(() => {
        userInteractingRef.current = false;
        userInteractingTimerRef.current = null;
      }, 220);
    };

    const handleScroll = () => {
      const atBottom = isNearBottom(el);
      isAtBottomRef.current = atBottom;
      if (programmaticScrollRef.current) {
        setShowScrollButton(false);
        return;
      }

      if (atBottom) {
        followModeRef.current = "following";
        userInteractingRef.current = false;
      } else if (userInteractingRef.current) {
        followModeRef.current = "paused";
      }
      setShowScrollButton(followModeRef.current === "paused" || !atBottom);
    };

    const handleUserScrollIntent = () => {
      markUserInteraction();
      window.requestAnimationFrame(updateBottomState);
    };

    el.addEventListener("wheel", handleUserScrollIntent, { passive: true });
    el.addEventListener("touchmove", handleUserScrollIntent, { passive: true });
    el.addEventListener("pointerdown", handleUserScrollIntent);
    el.addEventListener("keydown", handleUserScrollIntent);
    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => {
      el.removeEventListener("wheel", handleUserScrollIntent);
      el.removeEventListener("touchmove", handleUserScrollIntent);
      el.removeEventListener("pointerdown", handleUserScrollIntent);
      el.removeEventListener("keydown", handleUserScrollIntent);
      el.removeEventListener("scroll", handleScroll);
      if (userInteractingTimerRef.current !== null) {
        window.clearTimeout(userInteractingTimerRef.current);
      }
    };
  }, []);

  function isNearBottom(el: HTMLDivElement): boolean {
    return el.scrollHeight - el.scrollTop - el.clientHeight <= SCROLL_THRESHOLD;
  }

  function scrollToLatest(mode: "preserve" | "force") {
    const el = messageStreamRef.current;
    if (!el) return;
    if (mode === "preserve" && followModeRef.current !== "following") {
      setShowScrollButton(true);
      return;
    }
    if (mode === "force") {
      followModeRef.current = "following";
    }
    programmaticScrollRef.current = true;

    const scroll = () => {
      bottomRef.current?.scrollIntoView({ block: "end" });
      el.scrollTop = el.scrollHeight;
      isAtBottomRef.current = true;
      if (mode === "force" || followModeRef.current === "following") {
        followModeRef.current = "following";
      }
      userInteractingRef.current = false;
      setShowScrollButton(false);
    };

    scroll();
    window.requestAnimationFrame(() => {
      if (mode === "preserve" && followModeRef.current !== "following") {
        programmaticScrollRef.current = false;
        return;
      }
      scroll();
      window.requestAnimationFrame(() => {
        if (mode === "preserve" && followModeRef.current !== "following") {
          programmaticScrollRef.current = false;
          return;
        }
        scroll();
        window.setTimeout(() => {
          if (mode === "preserve" && followModeRef.current !== "following") {
            programmaticScrollRef.current = false;
            return;
          }
          scroll();
          programmaticScrollRef.current = false;
        }, 80);
      });
    });
  }

  useLayoutEffect(() => {
    if (!didRenderEntriesRef.current) {
      didRenderEntriesRef.current = true;
      return;
    }
    if (!isBusy) {
      return;
    }
    if (followModeRef.current === "following") {
      scrollToLatest("preserve");
    }
  }, [entries, isBusy]);

  useEffect(() => {
    const timeline = timelineRef.current;
    if (!timeline) return;
    const observer = new ResizeObserver(() => {
      if (isBusy && followModeRef.current === "following") {
        scrollToLatest("preserve");
      } else if (messageStreamRef.current) {
        setShowScrollButton(!isNearBottom(messageStreamRef.current));
      }
    });
    observer.observe(timeline);
    return () => observer.disconnect();
  }, [isBusy]);

  useEffect(() => {
    if (isBusy && !prevBusyRef.current) {
      isAtBottomRef.current = true;
      followModeRef.current = "following";
      scrollToLatest("force");
    }
    prevBusyRef.current = isBusy;
  }, [isBusy]);

  function scrollToBottom() {
    const el = messageStreamRef.current;
    if (el) {
      isAtBottomRef.current = true;
      followModeRef.current = "following";
      scrollToLatest("force");
      setShowScrollButton(false);
    }
  }

  return (
    <section className="conversation" data-status={status}>
      <header className="conversation-header">
        <div>
          <h1>{selectedSession?.title ?? t("conversation.defaultTitle")}</h1>
          <p>{selectedProject?.path ?? t("conversation.addProjectHint")}</p>
        </div>
      </header>

      <div className="message-stream" ref={messageStreamRef}>
        {entries.length === 0 ? (
          <div className="empty-state">
            <Terminal size={34} />
            <h2>{t("conversation.emptyTitle")}</h2>
            <p>{t("conversation.emptyDescription")}</p>
          </div>
        ) : (
          <div className="conversation-timeline" ref={timelineRef}>
            {entries.map((entry) => {
              if (entry.kind === "message") {
                return <MessageEntry key={entry.key} entry={entry} />;
              }
              if (entry.kind === "thought") {
                return <ThoughtEntry key={entry.key} content={entry.content} />;
              }
              if (entry.kind === "tool") {
                return <ToolEntry key={entry.key} toolCall={entry.toolCall} t={t} />;
              }
              if (entry.kind === "agent") {
                return <AgentEntry key={entry.key} activity={entry.activity} t={t} />;
              }
              return <TraceEntry key={entry.key} item={entry.item} t={t} />;
            })}
            <div className="timeline-bottom-anchor" ref={bottomRef} aria-hidden="true" />
          </div>
        )}
        {showScrollButton && (
          <button
            className="scroll-to-bottom"
            onClick={scrollToBottom}
            title={t("toolCall.scrollToBottom")}
            aria-label={t("toolCall.scrollToBottom")}
          >
            <ChevronDown size={18} />
          </button>
        )}
      </div>

      <footer className="conversation-footer">
        <SessionStatusBar
          runtime={sessionRuntime}
          providers={providers}
          roles={roles}
          setRoles={setRoles}
          onSaveProviderSettings={onSaveProviderSettings}
          turnPhase={turnPhase}
          turnStartedAt={turnStartedAt}
          agentActivities={agentActivities}
        />
        <div className="composer">
          <textarea
            value={prompt}
            disabled={!selectedSession || isBusy}
            onChange={(event) => onSetPrompt(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                onSendPrompt();
              }
            }}
            placeholder={selectedSession ? t("conversation.askPlaceholder") : t("conversation.noSessionPlaceholder")}
            aria-label={t("conversation.askPlaceholder")}
          />
          <button
            className={`send-button${isBusy ? " stop-button" : ""}`}
            disabled={isBusy ? stopping : !prompt.trim() || !selectedSession}
            onClick={isBusy ? onStopPrompt : onSendPrompt}
          >
            {isBusy ? <Square size={18} /> : <Send size={18} />}
            <span>{isBusy ? t(stopping ? "actions.stopping" : "actions.stop") : t("actions.send")}</span>
          </button>
        </div>
      </footer>
    </section>
  );
}
