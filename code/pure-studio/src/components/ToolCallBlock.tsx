import { ChevronRight, Loader, CheckCircle, XCircle, ShieldAlert, Wrench } from "lucide-react";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { ToolCallStatus, TrackedToolCall } from "../types";

const statusI18nKeys: Record<ToolCallStatus, string> = {
  streaming: "toolCall.streaming",
  completed: "toolCall.completed",
  pending_approval: "toolCall.pendingApproval",
  approved: "toolCall.approved",
  denied: "toolCall.denied",
  result_ready: "toolCall.resultReady",
};

function statusIcon(status: ToolCallStatus) {
  switch (status) {
    case "streaming":
      return <Loader size={12} className="spin" />;
    case "completed":
    case "approved":
    case "result_ready":
      return <CheckCircle size={12} />;
    case "pending_approval":
      return <ShieldAlert size={12} />;
    case "denied":
      return <XCircle size={12} />;
  }
}

function formatArguments(raw: string): string {
  if (!raw) return "";
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}

function toolIcon(_name: string) {
  // TODO: return specific icons per tool name (bash→Terminal, search→Search, etc.)
  return <Wrench size={14} />;
}

type ToolCallBlockProps = {
  toolCall: TrackedToolCall;
};

export function ToolCallBlock({ toolCall }: ToolCallBlockProps) {
  const { t } = useTranslation();
  const defaultExpanded = toolCall.status === "streaming" || toolCall.status === "pending_approval";
  const [expanded, setExpanded] = useState(defaultExpanded);

  return (
    <article className={`tool-call-block status-${toolCall.status}`}>
      <button className="tool-call-header" onClick={() => setExpanded((v) => !v)}>
        <span className="tool-call-icon">{toolIcon(toolCall.name)}</span>
        <span className="tool-call-name">{toolCall.name}</span>
        <span className={`tool-call-status-badge ${toolCall.status}`}>
          {statusIcon(toolCall.status)}
          {t(statusI18nKeys[toolCall.status])}
        </span>
        <ChevronRight size={14} className={`tool-call-chevron ${expanded ? "expanded" : ""}`} />
      </button>
      {expanded && (
        <div className="tool-call-body">
          {toolCall.arguments ? (
            <>
              <div className="tool-call-body-label">{t("toolCall.arguments")}</div>
              <pre className="tool-call-arguments">{formatArguments(toolCall.arguments)}</pre>
            </>
          ) : null}
          {toolCall.result ? (
            <>
              <div className="tool-call-body-label">{t("toolCall.result")}</div>
              <pre className="tool-call-result">{toolCall.result}</pre>
            </>
          ) : null}
        </div>
      )}
    </article>
  );
}
