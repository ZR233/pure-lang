import { Activity, Send, Terminal } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ChatMessage, SessionRecord, SubagentActivity, SubagentStatus, ProjectRecord } from "../types";
import { formatTime } from "../lib/utils";

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

type ConversationPanelProps = {
  selectedSession: SessionRecord | null;
  selectedProject: ProjectRecord | null;
  status: string;
  isBusy: boolean;
  liveMessages: ChatMessage[];
  subagentActivities: SubagentActivity[];
  prompt: string;
  onSetPrompt: (value: string) => void;
  onSendPrompt: () => void;
};

export function ConversationPanel({
  selectedSession,
  selectedProject,
  status,
  isBusy,
  liveMessages,
  subagentActivities,
  prompt,
  onSetPrompt,
  onSendPrompt,
}: ConversationPanelProps) {
  const { t } = useTranslation();

  return (
    <section className="conversation">
      <header className="conversation-header">
        <div>
          <h1>{selectedSession?.title ?? t("conversation.defaultTitle")}</h1>
          <p>{selectedProject?.path ?? t("conversation.addProjectHint")}</p>
        </div>
        <div className={`status-pill ${isBusy ? "running" : ""}`}>{status}</div>
      </header>

      <div className="message-stream">
        {liveMessages.length === 0 && subagentActivities.length === 0 ? (
          <div className="empty-state">
            <Terminal size={34} />
            <h2>{t("conversation.emptyTitle")}</h2>
            <p>{t("conversation.emptyDescription")}</p>
          </div>
        ) : (
          <>
            {liveMessages.map((message, index) => (
              <article key={`${message.role}-${index}`} className={`message ${message.role}`}>
                <div className="message-role">{message.role}</div>
                {message.reasoningContent ? (
                  <pre className="thinking-block">{message.reasoningContent}</pre>
                ) : null}
                <div className="message-content">{message.content}</div>
              </article>
            ))}
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
          </>
        )}
      </div>

      <footer className="composer">
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
        />
        <button
          className="send-button"
          disabled={!prompt.trim() || !selectedSession || isBusy}
          onClick={onSendPrompt}
        >
          <Send size={18} />
          <span>{isBusy ? t("status.running") : t("actions.send")}</span>
        </button>
      </footer>
    </section>
  );
}
