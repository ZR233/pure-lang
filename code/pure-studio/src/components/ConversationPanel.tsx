import { Activity, ChevronDown, Clock, Send, Terminal, Wrench } from "lucide-react";
import type { Dispatch, SetStateAction } from "react";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  ChatItem,
  ChatMessage,
  ProjectRecord,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  SubagentActivity,
  SubagentStatus,
  TimelineItem,
} from "../types";
import { formatTime } from "../lib/utils";
import { ToolCallBlock } from "./ToolCallBlock";
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

const statusClassNames: Record<SubagentStatus, string> = {
  queued: "queued",
  awaitingApproval: "awaiting-approval",
  running: "running",
  awaitingToolApproval: "awaiting-tool-approval",
  succeeded: "succeeded",
  failed: "failed",
  denied: "denied",
};

const roleI18nKeys: Record<string, string> = {
  explorer: "roles.explorer",
  planner: "roles.planner",
  executor: "roles.executor",
  reviewer: "roles.reviewer",
};

function subagentSummary(activity: SubagentActivity, t: (key: string) => string) {
  if (activity.error) {
    return activity.error;
  }
  if (activity.summary) {
    return activity.summary;
  }
  return activity.status === "queued" ? t("subagent.waitingToStart") : t("subagent.noSummaryYet");
}

function MessageBubble({ message }: { message: ChatMessage }) {
  return (
    <article className={`message ${message.role}`}>
      <div className="message-role">{message.role}</div>
      <div className="message-content">{message.content}</div>
    </article>
  );
}

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
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
  onSetPrompt: (value: string) => void;
  onSendPrompt: () => void;
};

const SCROLL_THRESHOLD = 40;

function formatUsage(usage?: { promptTokens: number; completionTokens: number; totalTokens: number } | null) {
  if (!usage) return null;
  return `${usage.promptTokens}p + ${usage.completionTokens}c = ${usage.totalTokens}t`;
}

function TimelineSection({ items }: { items: TimelineItem[] }) {
  const { t } = useTranslation();
  if (items.length === 0) return null;

  return (
    <section className="timeline-section" aria-label="Session timeline">
      <div className="subagent-timeline-head">
        <Clock size={16} />
        <span>{t("timeline.title", "Timeline")}</span>
      </div>
      {items.map((item) => {
        if (item.kind === "turn") {
          return (
            <article key={`tl-${item.sequence}`} className={`timeline-item turn status-${item.turnStatus ?? "started"}`}>
              <div className="timeline-item-header">
                <Activity size={14} />
                <span className="timeline-item-kind">
                  {t("timeline.turn", "Turn")}
                </span>
                <span className={`timeline-status status-${item.turnStatus}`}>
                  {item.turnStatus}
                </span>
              </div>
              {item.turnModel ? <p className="timeline-detail">{item.turnModel}</p> : null}
              {item.turnUsage ? (
                <p className="timeline-meta">{formatUsage(item.turnUsage)}</p>
              ) : null}
            </article>
          );
        }

        if (item.kind === "tool_call") {
          return (
            <article key={`tl-${item.sequence}`} className={`timeline-item tool-call status-${item.toolStatus ?? "started"}`}>
              <div className="timeline-item-header">
                <Wrench size={14} />
                <span className="timeline-item-kind">
                  {item.toolName ?? t("timeline.toolCall", "Tool Call")}
                </span>
                <span className={`timeline-status status-${item.toolStatus}`}>
                  {item.toolStatus}
                </span>
              </div>
              {item.toolArguments ? (
                <pre className="timeline-arguments">{item.toolArguments}</pre>
              ) : null}
              {item.toolResult ? (
                <p className="timeline-result">{item.toolResult}</p>
              ) : null}
            </article>
          );
        }

        return null;
      })}
    </section>
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
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
  onSetPrompt,
  onSendPrompt,
}: ConversationPanelProps) {
  const { t } = useTranslation();
  const messageStreamRef = useRef<HTMLDivElement>(null);
  const isAtBottomRef = useRef(true);
  const [showScrollButton, setShowScrollButton] = useState(false);
  const prevBusyRef = useRef(isBusy);

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
  }, [chatItems]);

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
    <section className="conversation">
      <header className="conversation-header">
        <div>
          <h1>{selectedSession?.title ?? t("conversation.defaultTitle")}</h1>
          <p>{selectedProject?.path ?? t("conversation.addProjectHint")}</p>
        </div>
      </header>

      <div className="message-stream" ref={messageStreamRef}>
        {chatItems.length === 0 && subagentActivities.length === 0 ? (
          <div className="empty-state">
            <Terminal size={34} />
            <h2>{t("conversation.emptyTitle")}</h2>
            <p>{t("conversation.emptyDescription")}</p>
          </div>
        ) : (
          <>
            {chatItems.map((item) =>
              item.kind === "message" ? (
                <MessageBubble key={item.key} message={item.message} />
              ) : (
                <ToolCallBlock key={item.key} toolCall={item.toolCall} />
              ),
            )}
            {subagentActivities.length > 0 ? (
              <section className="subagent-timeline" aria-label="Subagent activity">
                <div className="subagent-timeline-head">
                  <Activity size={16} />
                  <span>{t("subagent.title")}</span>
                </div>
                {subagentActivities.map((activity) => (
                  <article
                    key={activity.id}
                    className={`subagent-card status-${statusClassNames[activity.status]}`}
                    style={{ '--subagent-depth': Math.max(0, activity.depth - 1) } as React.CSSProperties}
                  >
                    <div className="subagent-card-head">
                      <span className="subagent-role">
                        {t(roleI18nKeys[activity.role] ?? `roles.${activity.role}`)}
                      </span>
                      <span className="subagent-status">
                        {t(subagentStatusKeys[activity.status])}
                      </span>
                    </div>
                    <p className="subagent-task">{activity.task}</p>
                    <p className="subagent-result">{subagentSummary(activity, t)}</p>
                    <div className="subagent-meta">
                      <span>{t("subagent.depth")} {activity.depth}</span>
                      {activity.parentId ? <span>{t("subagent.parent")} {activity.parentId}</span> : null}
                      <span>{formatTime(activity.updatedAt)}</span>
                    </div>
                  </article>
                ))}
              </section>
            ) : null}
            {timelineItems.length > 0 ? (
              <TimelineSection items={timelineItems} />
            ) : null}
          </>
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
          selectedSession={selectedSession}
          selectedProject={selectedProject}
          providers={providers}
          roles={roles}
          setRoles={setRoles}
          onSaveProviderSettings={onSaveProviderSettings}
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
            className="send-button"
            disabled={!prompt.trim() || !selectedSession || isBusy}
            onClick={onSendPrompt}
          >
            <Send size={18} />
            <span>{isBusy ? status : t("actions.send")}</span>
          </button>
        </div>
      </footer>
    </section>
  );
}
