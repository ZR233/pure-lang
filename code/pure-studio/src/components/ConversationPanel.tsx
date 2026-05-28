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
import { useEffect, useMemo, useRef, useState } from "react";
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
  failed: "subagent.failed",
  interrupted: "turnPhase.interrupted",
  closed: "status.done",
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
    case "failed":
      return t("turnPhase.failed");
    case "interrupted":
      return t("turnPhase.interrupted");
    default:
      return t("turnPhase.running");
  }
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
    } else if (item.kind === "turn" && (item.turnStatus === "failed" || item.turnStatus === "interrupted")) {
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
      <div className="timeline-message-content">{entry.content}</div>
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
  return (
    <EntryShell className={`timeline-entry-tool status-${toolCall.status}`} icon={<Wrench size={14} />}>
      <div className="timeline-entry-head">
        <strong>{toolCall.name}</strong>
        {toolCall.arguments ? (
          <code className="timeline-inline-code" title={toolCall.arguments}>
            {compact(toolCall.arguments, 180)}
          </code>
        ) : null}
        <span className={`timeline-badge status-${toolCall.status}`}>{toolStatusLabel(toolCall.status, t)}</span>
      </div>
      {toolCall.result ? <p className="timeline-result">{compact(toolCall.result, 520)}</p> : null}
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
      <p className="timeline-task">{activity.task}</p>
      <p className="timeline-result">{agentSummary(activity, t)}</p>
      <div className="timeline-entry-meta">
        <span>{t("subagent.depth")} {activity.depth}</span>
        {activity.parentPath ? <span>{t("subagent.parent")} {activity.parentPath}</span> : null}
        <span>{formatTime(activity.updatedAt)}</span>
      </div>
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
    return (
      <EntryShell className={`timeline-entry-tool status-${item.toolStatus ?? "started"}`} icon={<Wrench size={14} />}>
        <div className="timeline-entry-head">
          <strong>{item.toolName ?? "Tool call"}</strong>
          {item.toolArguments ? (
            <code className="timeline-inline-code" title={item.toolArguments}>
              {compact(item.toolArguments, 180)}
            </code>
          ) : null}
          <span className={`timeline-badge status-${item.toolStatus ?? "started"}`}>
            {toolStatusLabel(item.toolStatus, t)}
          </span>
        </div>
        {item.toolResult ? <p className="timeline-result">{compact(item.toolResult, 520)}</p> : null}
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
  const isAtBottomRef = useRef(true);
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

    const handleScroll = () => {
      const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < SCROLL_THRESHOLD;
      isAtBottomRef.current = atBottom;
      setShowScrollButton(!atBottom);
    };

    el.addEventListener("scroll", handleScroll, { passive: true });
    return () => el.removeEventListener("scroll", handleScroll);
  }, []);

  useEffect(() => {
    if (!didRenderEntriesRef.current) {
      didRenderEntriesRef.current = true;
      return;
    }
    if (!isBusy) {
      return;
    }
    if (isAtBottomRef.current && messageStreamRef.current) {
      messageStreamRef.current.scrollTop = messageStreamRef.current.scrollHeight;
    }
  }, [entries, isBusy]);

  useEffect(() => {
    if (isBusy && !prevBusyRef.current) {
      isAtBottomRef.current = true;
      if (messageStreamRef.current) {
        messageStreamRef.current.scrollTop = messageStreamRef.current.scrollHeight;
      }
    }
    prevBusyRef.current = isBusy;
  }, [isBusy]);

  function scrollToBottom() {
    const el = messageStreamRef.current;
    if (el) {
      el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
      isAtBottomRef.current = true;
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
          <div className="conversation-timeline">
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
          </div>
        )}
        {showScrollButton && (
          <button className="scroll-to-bottom" onClick={scrollToBottom}>
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
