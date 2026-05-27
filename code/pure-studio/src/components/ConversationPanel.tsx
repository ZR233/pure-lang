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
  ProjectRecord,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  SubagentActivity,
  SubagentStatus,
  TimelineItem,
  TrackedToolCall,
  TurnPhase,
} from "../types";
import { formatTime } from "../lib/utils";
import { SessionStatusBar } from "./SessionStatusBar";

const subagentStatusKeys: Record<SubagentStatus, string> = {
  queued: "subagent.queued",
  awaitingApproval: "subagent.awaitingApproval",
  running: "subagent.running",
  awaitingToolApproval: "subagent.awaitingTool",
  succeeded: "subagent.succeeded",
  failed: "subagent.failed",
  denied: "subagent.denied",
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
  | { kind: "subagent"; key: string; activity: SubagentActivity }
  | { kind: "trace"; key: string; item: TimelineItem };

type ConversationPanelProps = {
  selectedSession: SessionRecord | null;
  selectedProject: ProjectRecord | null;
  isBusy: boolean;
  chatItems: ChatItem[];
  subagentActivities: SubagentActivity[];
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

function subagentSummary(activity: SubagentActivity, t: TFunction) {
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

function timelineEntries(
  chatItems: ChatItem[],
  subagentActivities: SubagentActivity[],
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

  for (const activity of subagentActivities) {
    entries.push({
      kind: "subagent",
      key: `subagent-${activity.eventId}`,
      activity,
    });
  }

  for (const item of timelineItems) {
    if (item.kind === "tool_call" && item.toolCallId && seenToolIds.has(item.toolCallId)) {
      continue;
    }
    if (item.kind === "tool_call" || item.kind === "turn" || item.kind === "inference") {
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
      <div className="timeline-entry-head">
        <strong>{entry.role.toUpperCase()}</strong>
      </div>
      <div className="timeline-message-content">{entry.content}</div>
    </EntryShell>
  );
}

function ThoughtEntry({ content }: { content: string }) {
  return (
    <EntryShell className="timeline-entry-thought" icon={<Brain size={14} />}>
      <details className="timeline-thought">
        <summary>
          <span>Thought</span>
          <ChevronDown size={14} />
        </summary>
        <pre>{content}</pre>
      </details>
    </EntryShell>
  );
}

function ToolEntry({ toolCall }: { toolCall: TrackedToolCall }) {
  return (
    <EntryShell className={`timeline-entry-tool status-${toolCall.status}`} icon={<Wrench size={14} />}>
      <div className="timeline-entry-head">
        <strong>{toolCall.name}</strong>
        <span className={`timeline-badge status-${toolCall.status}`}>{toolCall.status}</span>
      </div>
      {toolCall.arguments ? (
        <pre className="timeline-code">{compact(toolCall.arguments, 520)}</pre>
      ) : null}
      {toolCall.result ? <p className="timeline-result">{compact(toolCall.result, 520)}</p> : null}
    </EntryShell>
  );
}

function SubagentEntry({
  activity,
  t,
}: {
  activity: SubagentActivity;
  t: TFunction;
}) {
  return (
    <EntryShell className={`timeline-entry-subagent status-${activity.status}`} icon={<Activity size={14} />}>
      <div className="timeline-entry-head">
        <strong>Agent: {t(roleI18nKeys[activity.role] ?? `roles.${activity.role}`)}</strong>
        <span className={`timeline-badge status-${activity.status}`}>
          {t(subagentStatusKeys[activity.status])}
        </span>
      </div>
      <p className="timeline-task">{activity.task}</p>
      <p className="timeline-result">{subagentSummary(activity, t)}</p>
      <div className="timeline-entry-meta">
        <span>{t("subagent.depth")} {activity.depth}</span>
        {activity.parentId ? <span>{t("subagent.parent")} {activity.parentId}</span> : null}
        <span>{formatTime(activity.updatedAt)}</span>
      </div>
    </EntryShell>
  );
}

function TraceEntry({ item }: { item: TimelineItem }) {
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
          <span className={`timeline-badge status-${item.toolStatus ?? "started"}`}>
            {item.toolStatus ?? "started"}
          </span>
        </div>
        {item.toolArguments ? <pre className="timeline-code">{compact(item.toolArguments, 520)}</pre> : null}
        {item.toolResult ? <p className="timeline-result">{compact(item.toolResult, 520)}</p> : null}
      </EntryShell>
    );
  }

  const icon = item.turnStatus === "completed" ? <CheckCircle2 size={14} /> : <Circle size={14} />;
  const usage = formatUsage(item.turnUsage);
  return (
    <EntryShell className={`timeline-entry-trace status-${item.turnStatus ?? "started"}`} icon={icon}>
      <div className="timeline-entry-head">
        <strong>Turn {item.turnStatus ?? "started"}</strong>
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
  subagentActivities,
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
  const [showScrollButton, setShowScrollButton] = useState(false);
  const prevBusyRef = useRef(isBusy);
  const entries = useMemo(
    () => timelineEntries(chatItems, subagentActivities, timelineItems),
    [chatItems, subagentActivities, timelineItems],
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
    if (isAtBottomRef.current && messageStreamRef.current) {
      messageStreamRef.current.scrollTop = messageStreamRef.current.scrollHeight;
    }
  }, [entries]);

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
                return <ToolEntry key={entry.key} toolCall={entry.toolCall} />;
              }
              if (entry.kind === "subagent") {
                return <SubagentEntry key={entry.key} activity={entry.activity} t={t} />;
              }
              return <TraceEntry key={entry.key} item={entry.item} />;
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
          subagentActivities={subagentActivities}
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
