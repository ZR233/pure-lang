import {
  Activity,
  ArrowLeft,
  ArrowRight,
  Brain,
  ChevronDown,
  Circle,
  CornerDownLeft,
  FileText,
  Loader2,
  Send,
  Square,
  Terminal,
  UserRound,
  Wrench,
} from "lucide-react";
import type { TFunction } from "i18next";
import type { Dispatch, KeyboardEvent, ReactNode, SetStateAction } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  AgentDto,
  AgentStatus,
  CompileMode,
  PermissionMode,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  TimelineItem,
  TurnPhase,
  ToolCallStatus2,
  UserInputRequest,
  UserInputResponse,
  UserQuestion,
} from "../types";
import type { TimelineEntry, ToolGroupSummaryPart } from "../state/selectors";
import type { PlanActionMode, PlanActionState } from "../state/studio-state";
import { hidesToolResult, isQuietFileTool } from "../lib/tool-display";
import { SessionStatusBar } from "./SessionStatusBar";
import { ConversationTimeline } from "./ConversationTimeline";

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

type ConversationPanelProps = {
  selectedSession: SessionRecord | null;
  isBusy: boolean;
  entries: TimelineEntry[];
  agents: AgentDto[];
  sessionRuntime: SessionRuntime | null;
  pendingUserInput: UserInputRequest | null;
  planAction: PlanActionState | null;
  prompt: string;
  status: string;
  turnPhase: TurnPhase;
  turnStartedAt: number | null;
  permissionMode: PermissionMode;
  providers: ProviderRecord[];
  roles: RoleRecord[];
  setRoles: Dispatch<SetStateAction<RoleRecord[]>>;
  onSaveProviderSettings: (explicitRoles?: RoleRecord[]) => void;
  onSavePermissionMode: (mode: PermissionMode) => void;
  onSetPrompt: (value: string) => void;
  onSetSessionMode: (mode: CompileMode) => void;
  onImplementPlan: (plan: string) => void;
  onSendPrompt: () => void;
  onSendPromptContent: (content: string) => void;
  onSetPlanActionMode: (mode: PlanActionMode) => void;
  onDismissPlanAction: () => void;
  onStopPrompt: () => void;
  onAnswerUserInput: (requestId: string, response: UserInputResponse) => void;
};

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

function thoughtLabel(content: string, t: TFunction): string {
  const firstLine = content.trim().split(/\r?\n/, 1)[0]?.trim();
  return firstLine?.toLowerCase().startsWith("thought") ? firstLine : t("timeline.thinking");
}

function isActiveStatus(status: TimelineItem["status"]): boolean {
  return status === "started" || status === "streaming" || status === "running";
}

function thoughtDurationLabel(seconds: number, t: TFunction): string {
  if (seconds <= 0) {
    return t("timeline.thoughtDurationSubSecond");
  }
  const minutes = Math.floor(seconds / 60);
  const remainingSeconds = seconds % 60;
  if (minutes > 0) {
    return t("timeline.thoughtDurationMinutes", { minutes, seconds: remainingSeconds });
  }
  return t("timeline.thoughtDurationSeconds", { seconds });
}

function toolStatusLabel(status: ToolCallStatus2 | null | undefined, t: TFunction): string {
  switch (status) {
    case "streaming":
    case "started":
    case "running":
      return t("toolCall.streaming");
    case "completed":
      return t("toolCall.completed");
    case "awaitingApproval":
      return t("toolCall.pendingApproval");
    case "approved":
      return t("toolCall.approved");
    case "denied":
      return t("toolCall.denied");
    case "failed":
      return t("subagent.failed");
    case "interrupted":
      return t("turnPhase.interrupted");
    case "budgetLimited":
      return t("turnPhase.budgetLimited");
    default:
      return t("toolCall.streaming");
  }
}

function turnStatusLabel(status: TimelineItem["status"], t: TFunction): string {
  switch (status) {
    case "started":
    case "streaming":
    case "running":
      return t("turnPhase.running");
    case "completed":
      return t("turnPhase.completed");
    case "denied":
      return t("toolCall.denied");
    case "failed":
      return t("turnPhase.failed");
    case "interrupted":
      return t("turnPhase.interrupted");
    case "budgetLimited":
      return t("turnPhase.budgetLimited");
    default:
      return t("turnPhase.running");
  }
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
  const command = args.command;
  if (name === "bash" && typeof command === "string" && command.trim()) {
    return compact(command, 90);
  }
  const from = args.from;
  const to = args.to;
  if (typeof from === "string" && from.trim() && typeof to === "string" && to.trim()) {
    return compact(`${from.trim()} -> ${to.trim()}`, 90);
  }
  const paths = args.paths;
  if (Array.isArray(paths)) {
    const summary = paths.filter(
      (path): path is string => typeof path === "string" && Boolean(path.trim()),
    );
    if (summary.length > 0) {
      return compact(summary.map((path) => path.trim()).join(", "), 90);
    }
  }
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
  return null;
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
  const roleIcon = entry.role === "user" ? <UserRound size={14} /> : <span className="timeline-avatar-letter">P</span>;
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

function ThoughtEntry({ entry, t }: { entry: Extract<TimelineEntry, { kind: "thought" }>; t: TFunction }) {
  const active = isActiveStatus(entry.status);
  return (
    <EntryShell className={`timeline-entry-thought status-${entry.status}${active ? " is-active" : ""}`} icon={<Brain size={14} />}>
      <details className="timeline-thought">
        <summary>
          <span className="timeline-thought-label">
            {active ? t("timeline.thinkingActive") : thoughtDurationLabel(entry.durationSeconds, t)}
          </span>
          {active ? <span className="timeline-thinking-dots" aria-hidden="true"><i /><i /><i /></span> : null}
          {!active && entry.content.trim() ? <span className="timeline-thought-preview">{thoughtLabel(entry.content, t)}</span> : null}
          <ChevronDown size={14} />
        </summary>
        <pre>{entry.content}</pre>
      </details>
    </EntryShell>
  );
}

function StatusEntry({ entry, t }: { entry: Extract<TimelineEntry, { kind: "status" }>; t: TFunction }) {
  return (
    <EntryShell className={`timeline-entry-status status-${entry.status}`} icon={<Loader2 size={14} />}>
      <div className="timeline-status-row">
        <span className="timeline-status-pulse" aria-hidden="true" />
        <span>{entry.content === "waitingForModel" ? t("timeline.waitingForModel") : entry.content}</span>
      </div>
    </EntryShell>
  );
}

function ToolEntry({ item, t }: { item: Extract<TimelineEntry, { kind: "tool" }>["item"]; t: TFunction }) {
  const tool = item.tool;
  const name = tool?.name || "Tool call";
  const argumentsText = tool?.arguments ?? "";
  const pathSummary = toolPathSummary(name, argumentsText);
  const hideResult = hidesToolResult(name, item.status);
  return (
    <EntryShell className={`timeline-entry-tool status-${item.status}`} icon={<Wrench size={14} />}>
      <div className="timeline-entry-head">
        <strong>{name}</strong>
        {pathSummary ? (
          <code className="timeline-inline-code" title={pathSummary}>
            {pathSummary}
          </code>
        ) : null}
        <span className={`timeline-badge status-${item.status}`}>{toolStatusLabel(item.status, t)}</span>
      </div>
      {!pathSummary && !isQuietFileTool(name) && argumentsText ? (
        <p className="timeline-result">{compact(argumentsText, 160)}</p>
      ) : null}
      <ToolDetails
        argumentsText={isQuietFileTool(name) ? null : argumentsText}
        result={tool?.result}
        hideResult={hideResult}
        t={t}
      />
    </EntryShell>
  );
}

function PlanEntry({
  entry,
  t,
}: {
  entry: Extract<TimelineEntry, { kind: "plan" }>;
  t: TFunction;
}) {
  return (
    <EntryShell className={`timeline-entry-plan status-${entry.item.status}`} icon={<FileText size={14} />}>
      <div className="timeline-entry-head">
        <strong>{t("timeline.plan")}</strong>
        <span className={`timeline-badge status-${entry.item.status}`}>{turnStatusLabel(entry.item.status, t)}</span>
      </div>
      <div className="timeline-message-content">
        <MarkdownContent content={entry.content} />
      </div>
    </EntryShell>
  );
}

function ToolGroupEntry({
  entry,
  t,
}: {
  entry: Extract<TimelineEntry, { kind: "toolGroup" }>;
  t: TFunction;
}) {
  return (
    <EntryShell className={`timeline-entry-tool-group status-${entry.status}`} icon={<Wrench size={14} />}>
      <div className="timeline-entry-head">
        <strong>{t("toolGroup.title")}</strong>
        <div className="timeline-tool-group-summary">
          {entry.summaryParts.map((part) => (
            <span key={part.kind} className={`timeline-tool-chip kind-${part.kind}`}>
              {toolGroupPartLabel(part, t)}
            </span>
          ))}
        </div>
        <span className={`timeline-badge status-${entry.status}`}>{toolStatusLabel(entry.status, t)}</span>
      </div>
      <details className="timeline-details timeline-tool-group-details">
        <summary>
          <span>{t("toolGroup.details", { count: entry.items.length })}</span>
          <ChevronDown size={14} />
        </summary>
        <div className="timeline-tool-group-list">
          {entry.items.map((item) => (
            <ToolGroupDetailRow key={item.itemId} item={item} t={t} />
          ))}
        </div>
      </details>
    </EntryShell>
  );
}

function toolGroupPartLabel(part: ToolGroupSummaryPart, t: TFunction): string {
  const suffix = part.count === 1 ? "One" : "";
  return t(`toolGroup.${part.kind}${suffix}`, { count: part.count });
}

function ToolGroupDetailRow({ item, t }: { item: TimelineItem; t: TFunction }) {
  const tool = item.tool;
  const name = tool?.name || "Tool call";
  const argumentsText = tool?.arguments ?? "";
  const pathSummary = toolPathSummary(name, argumentsText);
  const hideResult = hidesToolResult(name, item.status);
  return (
    <div className="timeline-tool-group-row">
      <div className="timeline-tool-group-row-head">
        <strong>{name}</strong>
        {pathSummary ? (
          <code className="timeline-inline-code" title={pathSummary}>
            {pathSummary}
          </code>
        ) : null}
        <span className={`timeline-badge status-${item.status}`}>{toolStatusLabel(item.status, t)}</span>
      </div>
      {!pathSummary && !isQuietFileTool(name) && argumentsText ? (
        <p className="timeline-result">{compact(argumentsText, 140)}</p>
      ) : null}
      <ToolDetails
        argumentsText={isQuietFileTool(name) ? null : argumentsText}
        result={tool?.result}
        hideResult={hideResult}
        t={t}
      />
    </div>
  );
}

function AgentEntry({
  item,
  t,
}: {
  item: Extract<TimelineEntry, { kind: "agent" }>["item"];
  t: TFunction;
}) {
  const agent = item.agent;
  const status = agent?.status ?? null;
  const prompt = agent?.task ?? null;
  const failureDetail = agentFailureDetail(agent, t);
  const summary = failureDetail ?? agent?.summary ?? "";
  const path = agent?.path ?? null;
  return (
    <EntryShell className={`timeline-entry-subagent status-${status ?? item.status}`} icon={<Activity size={14} />}>
      <div className="timeline-entry-head">
        <strong>{t("subagent.title")}</strong>
        {path ? <code className="timeline-inline-code">{path}</code> : null}
        {status ? (
          <span className={`timeline-badge status-${status}`}>{t(agentStatusKeys[status])}</span>
        ) : null}
      </div>
      <div className="timeline-entry-meta">
        {agent?.parentPath ? <span>{t("subagent.parent")} {agent.parentPath}</span> : null}
        <span>{new Date(item.updatedAt * 1000).toLocaleTimeString()}</span>
      </div>
      {prompt || summary ? (
        <details className="timeline-details">
          <summary>
            <span>{prompt ? t("subagent.prompt") : t("subagent.noSummaryYet")}</span>
            <ChevronDown size={14} />
          </summary>
          {prompt ? <p className="timeline-task">{prompt}</p> : null}
          {summary ? <p className="timeline-result">{summary}</p> : null}
        </details>
      ) : null}
    </EntryShell>
  );
}

function agentFailureDetail(
  agent: Extract<TimelineEntry, { kind: "agent" }>["item"]["agent"],
  t: TFunction,
): string | null {
  if (!agent || !["errored", "interrupted", "shutdown", "notFound"].includes(agent.status)) {
    return null;
  }
  if (agent.error?.trim()) {
    return agent.error;
  }
  if (!agent.reason?.trim()) {
    return null;
  }
  switch (agent.reason) {
    case "providerError":
      return t("subagent.providerError");
    case "toolError":
      return t("subagent.toolError");
    case "budgetLimited":
      return t("subagent.budgetLimited");
    case "interrupted":
      return t("subagent.interrupted");
    case "shutdown":
      return t("subagent.shutdown");
    default:
      return agent.reason;
  }
}

function TraceEntry({ item, t }: { item: TimelineItem; t: TFunction }) {
  const content = item.content.trim();
  return (
    <EntryShell className={`timeline-entry-trace status-${item.status ?? "started"}`} icon={<Circle size={14} />}>
      <div className="timeline-entry-head">
        <strong>{t("timeline.notice")} {turnStatusLabel(item.status, t)}</strong>
      </div>
      {content ? <p className="timeline-result">{compact(content, 260)}</p> : null}
    </EntryShell>
  );
}

type AskDraftEntry = {
  selected: string | null;
  text: string;
};

type AskDraft = Record<string, AskDraftEntry>;

function initialAskDraft(questions: UserQuestion[]): AskDraft {
  const draft: AskDraft = {};
  for (const question of questions) {
    draft[question.id] = {
      selected: null,
      text: "",
    };
  }
  return draft;
}

function AskUserComposer({
  request,
  stopping,
  onAnswer,
  t,
}: {
  request: UserInputRequest;
  stopping: boolean;
  onAnswer: (requestId: string, response: UserInputResponse) => void;
  t: TFunction;
}) {
  const [draft, setDraft] = useState<AskDraft>(() => initialAskDraft(request.questions));
  const [currentIndex, setCurrentIndex] = useState(0);
  const questionCount = request.questions.length;
  const currentQuestion = request.questions[currentIndex] ?? request.questions[0];
  const isFirst = currentIndex === 0;
  const isLast = currentIndex >= questionCount - 1;
  const progressLabel = useMemo(
    () => t("askUser.progress", { current: Math.min(currentIndex + 1, questionCount), total: questionCount }),
    [currentIndex, questionCount, t],
  );

  useEffect(() => {
    setDraft(initialAskDraft(request.questions));
    setCurrentIndex(0);
  }, [request.requestId, request.questions]);

  function updateDraft(id: string, update: Partial<AskDraftEntry>) {
    setDraft((current) => ({
      ...current,
      [id]: {
        selected: current[id]?.selected ?? null,
        text: current[id]?.text ?? "",
        ...update,
      },
    }));
  }

  function submit() {
    const answers: UserInputResponse["answers"] = {};
    for (const question of request.questions) {
      const entry = draft[question.id] ?? { selected: null, text: "" };
      const values: string[] = [];
      if (entry.selected) {
        values.push(entry.selected);
      }
      const text = entry.text.trim();
      if (text) {
        values.push(text);
      }
      answers[question.id] = { answers: values };
    }
    onAnswer(request.requestId, { answers });
  }

  function submitOnShortcut(event: KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      if (isLast) {
        submit();
      } else {
        setCurrentIndex((value) => Math.min(value + 1, questionCount - 1));
      }
    }
  }

  if (!currentQuestion) {
    return null;
  }

  const options = currentQuestion.options ?? [];
  const entry = draft[currentQuestion.id] ?? { selected: null, text: "" };
  const showFreeText = options.length === 0 || currentQuestion.isOther;

  return (
    <div className="composer-box ask-user-composer">
      <div className="ask-user-heading">
        <UserRound size={16} />
        <span>{t("askUser.awaiting")}</span>
        <small>{progressLabel}</small>
      </div>
      <div className="ask-user-question-list">
        <section className="ask-user-question" key={currentQuestion.id}>
          <div className="ask-user-question-copy">
            <strong>{currentQuestion.header}</strong>
            <p>{currentQuestion.question}</p>
          </div>
          {options.length > 0 ? (
            <div className="ask-user-options">
              {options.map((option) => (
                <button
                  type="button"
                  key={`${currentQuestion.id}-${option.label}`}
                  className={entry.selected === option.label ? "selected" : ""}
                  onClick={() =>
                    updateDraft(currentQuestion.id, {
                      selected: entry.selected === option.label ? null : option.label,
                    })
                  }
                >
                  <span>{option.label}</span>
                  <small>{option.description}</small>
                </button>
              ))}
            </div>
          ) : null}
          {showFreeText ? (
            currentQuestion.isSecret ? (
              <input
                type="password"
                value={entry.text}
                onChange={(event) => updateDraft(currentQuestion.id, { text: event.target.value })}
                onKeyDown={submitOnShortcut}
                placeholder={t("askUser.secretPlaceholder")}
                aria-label={currentQuestion.question}
              />
            ) : (
              <textarea
                value={entry.text}
                onChange={(event) => updateDraft(currentQuestion.id, { text: event.target.value })}
                onKeyDown={submitOnShortcut}
                placeholder={
                  options.length > 0
                    ? t("askUser.customPlaceholder")
                    : t("askUser.answerPlaceholder")
                }
                aria-label={currentQuestion.question}
              />
            )
          ) : null}
        </section>
      </div>
      <div className="composer-toolbar ask-user-toolbar">
        <span className="shortcut-hint">
          <kbd>⌘</kbd> + <kbd>Enter</kbd>
        </span>
        <div className="ask-user-toolbar-actions">
          <button
            type="button"
            onClick={() => setCurrentIndex((value) => Math.max(0, value - 1))}
            disabled={stopping || isFirst}
          >
            <ArrowLeft size={14} />
            <span>{t("actions.back")}</span>
          </button>
          <button
            type="button"
            onClick={() => {
              if (isLast) {
                submit();
              } else {
                setCurrentIndex((value) => Math.min(questionCount - 1, value + 1));
              }
            }}
            disabled={stopping}
          >
            {isLast ? <Send size={14} /> : <ArrowRight size={14} />}
            <span>{isLast ? t("askUser.submit") : t("askUser.next")}</span>
          </button>
        </div>
      </div>
    </div>
  );
}

function PlanConfirmComposer({
  action,
  stopping,
  onImplementPlan,
  onSendPromptContent,
  onSetMode,
  onCancel,
  t,
}: {
  action: PlanActionState;
  stopping: boolean;
  onImplementPlan: (plan: string) => void;
  onSendPromptContent: (content: string) => void;
  onSetMode: (mode: PlanActionMode) => void;
  onCancel: () => void;
  t: TFunction;
}) {
  const [message, setMessage] = useState("");
  const composerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const discussing = action.mode === "discuss";

  useEffect(() => {
    setMessage("");
  }, [action.planId, action.mode]);

  useEffect(() => {
    if (discussing) {
      textareaRef.current?.focus();
    } else {
      composerRef.current?.focus();
    }
  }, [action.planId, discussing]);

  function submitDiscussion() {
    if (stopping) return;
    const content = message.trim();
    if (!content) return;
    onCancel();
    onSendPromptContent(content);
  }

  function submitSelected() {
    if (stopping) return;
    if (discussing) {
      submitDiscussion();
      return;
    }
    if (!action.content.trim()) return;
    onCancel();
    onImplementPlan(action.content);
  }

  function submitOnShortcut(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      submitDiscussion();
    }
  }

  function handleComposerKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      if (!stopping) {
        onCancel();
      }
      return;
    }

    if (
      event.target instanceof HTMLTextAreaElement ||
      event.target instanceof HTMLButtonElement ||
      event.target instanceof HTMLInputElement ||
      event.target instanceof HTMLSelectElement
    ) {
      return;
    }

    if (event.key === "1") {
      event.preventDefault();
      onSetMode("choice");
      return;
    }
    if (event.key === "2") {
      event.preventDefault();
      onSetMode("discuss");
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      submitSelected();
    }
  }

  return (
    <div
      ref={composerRef}
      className={`composer-box plan-confirm-composer${discussing ? " discussing" : ""}`}
      onKeyDown={handleComposerKeyDown}
      tabIndex={-1}
    >
      <div className="plan-choice-title">{t("planConfirm.promptTitle")}</div>
      <div className="plan-choice-list" role="radiogroup" aria-label={t("planConfirm.promptTitle")}>
        <button
          type="button"
          className={`plan-choice-row${discussing ? "" : " selected"}`}
          role="radio"
          aria-checked={!discussing}
          onClick={() => onSetMode("choice")}
          disabled={stopping}
        >
          <span className="plan-choice-index">1.</span>
          <span className="plan-choice-copy">{t("planConfirm.implementChoice")}</span>
        </button>
        <button
          type="button"
          className={`plan-choice-row${discussing ? " selected" : ""}`}
          role="radio"
          aria-checked={discussing}
          onClick={() => onSetMode("discuss")}
          disabled={stopping}
        >
          <span className="plan-choice-index">2.</span>
          <span className="plan-choice-copy">{t("planConfirm.adjustChoice")}</span>
        </button>
      </div>
      {discussing ? (
        <textarea
          ref={textareaRef}
          className="plan-choice-textarea"
          value={message}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={submitOnShortcut}
          placeholder={t("planConfirm.discussPlaceholder")}
          aria-label={t("planConfirm.discussPlaceholder")}
        />
      ) : null}
      <div className="composer-toolbar plan-choice-toolbar">
        <span className="shortcut-hint">
          {discussing ? <><kbd>⌘</kbd> + <kbd>Enter</kbd></> : null}
        </span>
        <div className="plan-choice-toolbar-actions">
          <button type="button" className="plan-choice-ignore" onClick={onCancel} disabled={stopping}>
            <span>{t("planConfirm.ignore")}</span>
            <kbd>ESC</kbd>
          </button>
          <button
            type="button"
            className="plan-choice-submit"
            onClick={submitSelected}
            disabled={stopping || (discussing ? !message.trim() : !action.content.trim())}
          >
            <span>{t("planConfirm.submit")}</span>
            <CornerDownLeft size={13} />
          </button>
        </div>
      </div>
    </div>
  );
}

export function ConversationPanel({
  selectedSession,
  isBusy,
  entries,
  agents,
  sessionRuntime,
  pendingUserInput,
  planAction,
  prompt,
  status,
  turnPhase,
  turnStartedAt,
  permissionMode,
  providers,
  roles,
  setRoles,
  onSaveProviderSettings,
  onSavePermissionMode,
  onSetPrompt,
  onSetSessionMode,
  onImplementPlan,
  onSendPrompt,
  onSendPromptContent,
  onSetPlanActionMode,
  onDismissPlanAction,
  onStopPrompt,
  onAnswerUserInput,
}: ConversationPanelProps) {
  const { t } = useTranslation();
  const stopping = turnPhase === "stopping";
  const currentMode: CompileMode = selectedSession?.mode === "plan" ? "plan" : "auto";
  const activePlanAction = isBusy ? null : planAction;
  const composerHasInlineActions = Boolean(pendingUserInput || activePlanAction);

  function renderTimelineEntry(entry: TimelineEntry) {
    if (entry.kind === "message") {
      return <MessageEntry entry={entry} />;
    }
    if (entry.kind === "plan") {
      return <PlanEntry entry={entry} t={t} />;
    }
    if (entry.kind === "thought") {
      return <ThoughtEntry entry={entry} t={t} />;
    }
    if (entry.kind === "status") return <StatusEntry entry={entry} t={t} />;
    if (entry.kind === "tool") {
      return <ToolEntry item={entry.item} t={t} />;
    }
    if (entry.kind === "toolGroup") {
      return <ToolGroupEntry entry={entry} t={t} />;
    }
    if (entry.kind === "agent") {
      return <AgentEntry item={entry.item} t={t} />;
    }
    return <TraceEntry item={entry.item} t={t} />;
  }

  return (
    <section className="conversation" data-status={status}>
      <header className="conversation-header">
        <h1>{selectedSession?.title ?? t("conversation.defaultTitle")}</h1>
      </header>

      <ConversationTimeline
        sessionId={selectedSession?.id ?? null}
        entries={entries}
        isBusy={isBusy}
        scrollToBottomLabel={t("toolCall.scrollToBottom")}
        renderEntry={renderTimelineEntry}
        emptyState={
          <div className="empty-state">
            <Terminal size={34} />
            <h2>{t("conversation.emptyTitle")}</h2>
            <p>{t("conversation.emptyDescription")}</p>
          </div>
        }
      />

      <footer className="conversation-footer">
        <div className={`composer${composerHasInlineActions ? " composer-inline-action" : ""}`}>
          {pendingUserInput ? (
            <AskUserComposer
              request={pendingUserInput}
              stopping={stopping}
              onAnswer={onAnswerUserInput}
              t={t}
            />
          ) : activePlanAction ? (
            <PlanConfirmComposer
              action={activePlanAction}
              stopping={stopping}
              onImplementPlan={onImplementPlan}
              onSendPromptContent={onSendPromptContent}
              onSetMode={onSetPlanActionMode}
              onCancel={onDismissPlanAction}
              t={t}
            />
          ) : (
            <div className="composer-box">
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
              <div className="composer-toolbar">
                <span className="shortcut-hint"><kbd>⌘</kbd> + <kbd>Enter</kbd></span>
                <span />
              </div>
            </div>
          )}
          {composerHasInlineActions ? null : (
            <button
              className={`send-button${isBusy ? " stop-button" : ""}`}
              disabled={isBusy ? stopping : !prompt.trim() || !selectedSession}
              onClick={isBusy ? onStopPrompt : onSendPrompt}
            >
              {isBusy ? <Square size={18} /> : <Send size={18} />}
            </button>
          )}
        </div>

        <SessionStatusBar
          runtime={sessionRuntime}
          providers={providers}
          roles={roles}
          setRoles={setRoles}
          onSaveProviderSettings={onSaveProviderSettings}
          onSavePermissionMode={onSavePermissionMode}
          onSetSessionMode={onSetSessionMode}
          currentMode={currentMode}
          isBusy={isBusy}
          selectedSession={selectedSession}
          turnPhase={turnPhase}
          turnStartedAt={turnStartedAt}
          permissionMode={permissionMode}
          agents={agents}
        />
      </footer>
    </section>
  );
}
