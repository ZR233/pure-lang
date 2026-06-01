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
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type {
  AgentDto,
  AgentStatus,
  ProjectRecord,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  TimelineItem,
  TurnPhase,
  ToolCallStatus2,
} from "../types";
import type { TimelineEntry } from "../state/selectors";
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

type ConversationPanelProps = {
  selectedSession: SessionRecord | null;
  selectedProject: ProjectRecord | null;
  isBusy: boolean;
  entries: TimelineEntry[];
  agents: AgentDto[];
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

function formatUsage(usage?: { promptTokens: number; completionTokens: number; totalTokens: number } | null) {
  if (!usage) return null;
  return `${usage.promptTokens}p + ${usage.completionTokens}c = ${usage.totalTokens}t`;
}

function thoughtLabel(content: string): string {
  const firstLine = content.trim().split(/\r?\n/, 1)[0]?.trim();
  return firstLine?.toLowerCase().startsWith("thought") ? firstLine : "Thought";
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

function ToolEntry({ item, t }: { item: Extract<TimelineEntry, { kind: "tool" }>["item"]; t: TFunction }) {
  const tool = item.tool;
  const name = tool?.name ?? "Tool call";
  const argumentsText = tool?.arguments ?? "";
  const pathSummary = toolPathSummary(name, argumentsText);
  const hideResult = hidesToolResult(name);
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
  if (item.kind === "inference") {
    const usage = formatUsage(item.usage);
    return (
      <EntryShell className="timeline-entry-trace status-inference" icon={<Clock size={14} />}>
        <div className="timeline-entry-head">
          <strong>{item.inference?.model ?? "Inference"}</strong>
          {usage ? <span className="timeline-entry-meta-inline">{usage}</span> : null}
        </div>
      </EntryShell>
    );
  }
  const icon = item.status === "completed" ? <CheckCircle2 size={14} /> : <Circle size={14} />;
  const usage = formatUsage(item.usage);
  return (
    <EntryShell className={`timeline-entry-trace status-${item.status ?? "started"}`} icon={icon}>
      <div className="timeline-entry-head">
        <strong>{t("timeline.turn")} {turnStatusLabel(item.status, t)}</strong>
      </div>
      {usage ? <p className="timeline-entry-meta">{usage}</p> : null}
      {item.content ? <p className="timeline-result">{compact(item.content, 260)}</p> : null}
    </EntryShell>
  );
}

export function ConversationPanel({
  selectedSession,
  selectedProject,
  isBusy,
  entries,
  agents,
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
    if (typeof ResizeObserver === "undefined") {
      if (isBusy && followModeRef.current === "following") {
        scrollToLatest("preserve");
      }
      return;
    }
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
                return <ToolEntry key={entry.key} item={entry.item} t={t} />;
              }
              if (entry.kind === "agent") {
                return <AgentEntry key={entry.key} item={entry.item} t={t} />;
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
          agents={agents}
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
