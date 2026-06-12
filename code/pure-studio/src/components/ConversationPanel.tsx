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
  LspServerRecord,
  PermissionMode,
  InteractionRequest,
  InteractionResolution,
  PlanLifecycleState,
  ProviderRecord,
  RoleRecord,
  SessionRecord,
  SessionRuntime,
  TimelineItem,
  TurnPhase,
  ToolCallStatus2,
  UserInputResponse,
  UserQuestion,
} from "../types";
import type { TimelineEntry, ToolGroupSummaryPart } from "../state/selectors";
import { hidesToolResult, isQuietFileTool } from "../lib/tool-display";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Textarea } from "@/components/ui/textarea";
import { SessionStatusBar } from "./SessionStatusBar";
import { ConversationTimeline } from "./ConversationTimeline";
import { MarkdownContent } from "./MarkdownContent";

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
  lspServers: LspServerRecord[];
  activeInteraction: InteractionRequest | null;
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
  onImplementPlanFresh: (interactionId: string) => Promise<boolean> | boolean;
  onDiscussPlan: (planId: string, content: string) => Promise<boolean> | boolean;
  onSendPrompt: () => void;
  onDismissPlanAction: (planId: string) => void;
  onStopPrompt: () => void;
  onResolveInteraction: (interactionId: string, resolution: InteractionResolution) => void;
};

function compact(value: string, max = 220): string {
  const text = value.trim();
  if (text.length <= max) {
    return text;
  }
  return `${text.slice(0, max)}...`;
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

function planStateLabel(
  state: PlanLifecycleState | "pending",
  t: TFunction,
): string {
  switch (state) {
    case "accepted":
      return t("planState.accepted");
    case "pendingConfirmation":
      return t("planState.pendingConfirmation");
    case "implementing":
      return t("planState.implementing");
    case "implemented":
      return t("planState.implemented");
    case "implementationFailed":
      return t("planState.implementationFailed");
    case "continuedPlanning":
      return t("planState.continuedPlanning");
    case "dismissed":
      return t("planState.dismissed");
    case "cancelled":
      return t("planState.cancelled");
    case "pending":
      return t("planState.pending");
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
    <details className="text-xs text-muted-foreground mt-1 group">
      <summary className="flex items-center gap-1 cursor-pointer hover:text-foreground transition-colors">
        <span>{t("toolCall.details")}</span>
        <ChevronDown size={14} className="group-open:rotate-180 transition-transform" />
      </summary>
      {hasArguments ? (
        <>
          <div className="text-xs font-medium text-muted-foreground mt-2 mb-1">{t("toolCall.arguments")}</div>
          <pre className="bg-muted rounded-md p-2 mt-1 text-xs overflow-x-auto">{argumentsText}</pre>
        </>
      ) : null}
      {hasResult ? (
        <>
          <div className="text-xs font-medium text-muted-foreground mt-2 mb-1">{t("toolCall.result")}</div>
          <pre className="bg-muted rounded-md p-2 mt-1 text-xs overflow-x-auto">{result}</pre>
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
    <article className={`flex w-full gap-2.5 px-4 py-3 ${className}`}>
      <span className="w-7 h-7 rounded-full bg-primary/10 flex items-center justify-center shrink-0" aria-hidden="true">
        {icon}
      </span>
      <div className="flex-1 min-w-0">{children}</div>
    </article>
  );
}

function MessageEntry({ entry }: { entry: Extract<TimelineEntry, { kind: "message" }> }) {
  const roleIcon = entry.role === "user" ? <UserRound size={14} /> : <span className="text-xs font-bold text-primary">P</span>;
  if (entry.role === "user") {
    return (
      <div className="flex w-full gap-2.5 px-4 py-3">
        <span className="w-7 h-7 rounded-full bg-primary/10 flex items-center justify-center shrink-0" aria-hidden="true">
          {roleIcon}
        </span>
        <div className="bg-muted rounded-2xl rounded-tl-sm px-4 py-2.5 max-w-[80%]">
          <div className="text-sm whitespace-pre-wrap break-words">
            <MarkdownContent content={entry.content} />
          </div>
        </div>
      </div>
    );
  }
  return (
    <EntryShell className="" icon={roleIcon}>
      <div className="flex items-center gap-2 mb-1">
        <strong className="text-xs text-muted-foreground">{entry.role.toUpperCase()}</strong>
      </div>
      <div className="text-sm whitespace-pre-wrap break-words">
        <MarkdownContent content={entry.content} />
      </div>
    </EntryShell>
  );
}

function ThoughtEntry({ entry, t }: { entry: Extract<TimelineEntry, { kind: "thought" }>; t: TFunction }) {
  const active = isActiveStatus(entry.status);
  return (
    <div className="flex w-full items-center gap-2 text-sm text-muted-foreground bg-muted/30 rounded-md px-3 py-1.5 my-1">
      {active ? <Loader2 size={14} className="animate-spin shrink-0" /> : <Brain size={14} className="shrink-0" />}
      <details className="flex-1 min-w-0 group">
        <summary className="flex items-center gap-1 cursor-pointer list-none">
          <span className="flex-1 min-w-0">
            {active ? t("timeline.thinkingActive") : thoughtDurationLabel(entry.durationSeconds, t)}
          </span>
          {active ? (
            <span className="inline-flex gap-0.5" aria-hidden="true">
              <span className="w-1 h-1 rounded-full bg-current animate-bounce [animation-delay:-0.3s]" />
              <span className="w-1 h-1 rounded-full bg-current animate-bounce [animation-delay:-0.15s]" />
              <span className="w-1 h-1 rounded-full bg-current animate-bounce" />
            </span>
          ) : null}
          {!active && entry.content.trim() ? (
            <span className="text-xs truncate max-w-[200px] text-muted-foreground/70 ml-2">
              {thoughtLabel(entry.content, t)}
            </span>
          ) : null}
          <ChevronDown size={12} className="shrink-0 group-open:rotate-180 transition-transform" />
        </summary>
        <pre className="text-xs mt-2 whitespace-pre-wrap">{entry.content}</pre>
      </details>
    </div>
  );
}

function StatusEntry({ entry, t }: { entry: Extract<TimelineEntry, { kind: "status" }>; t: TFunction }) {
  return (
    <EntryShell className="" icon={<Loader2 size={14} className="animate-spin" />}>
      <div className="flex items-center gap-2 text-sm text-muted-foreground">
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
    <EntryShell className="" icon={<Wrench size={14} />}>
      <div className="flex items-center gap-2 text-sm text-muted-foreground flex-wrap">
        <Badge variant="outline" className="text-xs font-mono">{name}</Badge>
        {pathSummary ? (
          <code className="bg-muted rounded px-1 py-0.5 text-xs font-mono truncate max-w-[200px]" title={pathSummary}>
            {pathSummary}
          </code>
        ) : null}
        <Badge variant="secondary" className="text-xs">{toolStatusLabel(item.status, t)}</Badge>
      </div>
      {!pathSummary && !isQuietFileTool(name) && argumentsText ? (
        <p className="text-xs text-muted-foreground mt-0.5">{compact(argumentsText, 160)}</p>
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
  const state = entry.planState?.state ?? "pending";
  return (
    <div className="flex w-full gap-2.5 px-4 py-3">
      <span className="w-7 h-7 rounded-full bg-primary/10 flex items-center justify-center shrink-0" aria-hidden="true">
        <FileText size={14} />
      </span>
      <div className="flex-1 min-w-0">
        <Card className="w-full">
          <CardHeader className="flex flex-row items-center gap-2 p-3">
            <CardTitle className="text-sm">{t("timeline.plan")}</CardTitle>
            <Badge variant="secondary">{planStateLabel(state, t)}</Badge>
          </CardHeader>
          <CardContent className="p-3 pt-0">
            <MarkdownContent content={entry.content} />
          </CardContent>
        </Card>
      </div>
    </div>
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
    <EntryShell className="" icon={<Wrench size={14} />}>
      <div className="flex items-center gap-2 flex-wrap">
        <strong className="text-sm">{t("toolGroup.title")}</strong>
        <div className="flex items-center gap-1.5 text-sm text-muted-foreground">
          {entry.summaryParts.map((part) => (
            <Badge key={part.kind} variant="secondary" className="text-xs">
              {toolGroupPartLabel(part, t)}
            </Badge>
          ))}
        </div>
        <Badge variant="outline" className="text-xs">{toolStatusLabel(entry.status, t)}</Badge>
      </div>
      <details className="text-xs text-muted-foreground mt-1 group">
        <summary className="flex items-center gap-1 cursor-pointer hover:text-foreground transition-colors">
          <span>{t("toolGroup.details", { count: entry.items.length })}</span>
          <ChevronDown size={14} className="group-open:rotate-180 transition-transform" />
        </summary>
        <div className="space-y-0.5 mt-1">
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
    <div className="py-1">
      <div className="flex items-center gap-2 text-sm text-muted-foreground flex-wrap">
        <strong className="text-xs font-medium">{name}</strong>
        {pathSummary ? (
          <code className="bg-muted rounded px-1 py-0.5 text-xs font-mono truncate max-w-[200px]" title={pathSummary}>
            {pathSummary}
          </code>
        ) : null}
        <Badge variant="outline" className="text-xs">{toolStatusLabel(item.status, t)}</Badge>
      </div>
      {!pathSummary && !isQuietFileTool(name) && argumentsText ? (
        <p className="text-xs text-muted-foreground mt-0.5">{compact(argumentsText, 140)}</p>
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
    <EntryShell className="" icon={<Activity size={14} />}>
      <div className="flex items-center gap-2 flex-wrap">
        <strong className="text-sm">{t("subagent.title")}</strong>
        {path ? <code className="bg-muted rounded px-1 py-0.5 text-xs font-mono">{path}</code> : null}
        {status ? (
          <Badge variant="outline" className="text-xs flex items-center gap-1">
            <span className={`w-1.5 h-1.5 rounded-full ${status === "running" ? "bg-green-500" : status === "errored" ? "bg-destructive" : "bg-muted-foreground"}`} />
            {t(agentStatusKeys[status])}
          </Badge>
        ) : null}
      </div>
      <div className="flex items-center gap-2 text-xs text-muted-foreground mt-0.5">
        {agent?.parentPath ? <span>{t("subagent.parent")} {agent.parentPath}</span> : null}
        <span>{new Date(item.updatedAt * 1000).toLocaleTimeString()}</span>
      </div>
      {prompt || summary ? (
        <details className="text-xs text-muted-foreground mt-1 group">
          <summary className="flex items-center gap-1 cursor-pointer hover:text-foreground transition-colors">
            <span>{prompt ? t("subagent.prompt") : t("subagent.noSummaryYet")}</span>
            <ChevronDown size={14} className="group-open:rotate-180 transition-transform" />
          </summary>
          {prompt ? <MarkdownContent content={prompt} className="mt-1 text-xs" /> : null}
          {summary ? <MarkdownContent content={summary} className="mt-1 text-xs text-muted-foreground" /> : null}
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
    <EntryShell className="" icon={<Circle size={14} />}>
      <div className="flex items-center gap-2">
        <strong className="text-sm">{t("timeline.notice")}</strong>
        <Badge variant="outline" className="text-xs">{turnStatusLabel(item.status, t)}</Badge>
      </div>
      {content ? <p className="text-xs text-muted-foreground mt-0.5">{compact(content, 260)}</p> : null}
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
  interactionId,
  questions,
  stopping,
  onAnswer,
  t,
}: {
  interactionId: string;
  questions: UserQuestion[];
  stopping: boolean;
  onAnswer: (interactionId: string, response: UserInputResponse) => void;
  t: TFunction;
}) {
  const [draft, setDraft] = useState<AskDraft>(() => initialAskDraft(questions));
  const [currentIndex, setCurrentIndex] = useState(0);
  const questionCount = questions.length;
  const currentQuestion = questions[currentIndex] ?? questions[0];
  const isFirst = currentIndex === 0;
  const isLast = currentIndex >= questionCount - 1;
  const progressLabel = useMemo(
    () => t("askUser.progress", { current: Math.min(currentIndex + 1, questionCount), total: questionCount }),
    [currentIndex, questionCount, t],
  );

  useEffect(() => {
    setDraft(initialAskDraft(questions));
    setCurrentIndex(0);
  }, [interactionId, questions]);

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
    for (const question of questions) {
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
    onAnswer(interactionId, { answers });
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
    <div className="border-t border-border">
      <div className="flex items-center gap-2 px-4 pt-3 pb-2">
        <UserRound size={16} className="shrink-0" />
        <span className="text-sm font-medium">{t("askUser.awaiting")}</span>
        <span className="text-xs text-muted-foreground">{progressLabel}</span>
      </div>
      <div className="px-4 py-2">
        <section className="space-y-2" key={currentQuestion.id}>
          <div className="space-y-1">
            <strong className="text-sm">{currentQuestion.header}</strong>
            <p className="text-sm text-muted-foreground">{currentQuestion.question}</p>
          </div>
          {options.length > 0 ? (
            <div className="flex flex-col gap-1">
              {options.map((option) => (
                <Button
                  type="button"
                  key={`${currentQuestion.id}-${option.label}`}
                  variant={entry.selected === option.label ? "default" : "outline"}
                  className="w-full text-left h-auto py-2 justify-start"
                  onClick={() =>
                    updateDraft(currentQuestion.id, {
                      selected: entry.selected === option.label ? null : option.label,
                    })
                  }
                >
                  <span>{option.label}</span>
                  <span className="text-xs text-muted-foreground ml-2 font-normal">{option.description}</span>
                </Button>
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
                className="flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm shadow-sm transition-colors placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
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
                className="flex min-h-[60px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
              />
            )
          ) : null}
        </section>
      </div>
      <div className="flex items-center justify-between px-4 pb-3">
        <span className="text-xs text-muted-foreground">
          <kbd className="px-1 py-0.5 rounded border border-border bg-muted text-[10px] font-mono">⌘</kbd>
          {" + "}
          <kbd className="px-1 py-0.5 rounded border border-border bg-muted text-[10px] font-mono">Enter</kbd>
        </span>
        <div className="flex items-center gap-2">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => setCurrentIndex((value) => Math.max(0, value - 1))}
            disabled={stopping || isFirst}
          >
            <ArrowLeft size={14} />
            <span>{t("actions.back")}</span>
          </Button>
          <Button
            type="button"
            size="sm"
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
          </Button>
        </div>
      </div>
    </div>
  );
}

function PlanConfirmComposer({
  interactionId,
  stopping,
  onImplementPlanFresh,
  onDiscussPlan,
  onCancel,
  t,
}: {
  interactionId: string;
  stopping: boolean;
  onImplementPlanFresh: (interactionId: string) => Promise<boolean> | boolean;
  onDiscussPlan: (interactionId: string, content: string) => Promise<boolean> | boolean;
  onCancel: (interactionId: string) => void;
  t: TFunction;
}) {
  const [message, setMessage] = useState("");
  const [submittingAction, setSubmittingAction] = useState<"fresh" | "discuss" | null>(null);
  const composerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const submitting = submittingAction !== null;

  useEffect(() => {
    setMessage("");
    setSubmittingAction(null);
  }, [interactionId]);

  useEffect(() => {
    textareaRef.current?.focus();
  }, [interactionId]);

  function submitDiscussion() {
    if (stopping || submitting) return;
    const content = message.trim();
    if (!content) return;
    setSubmittingAction("discuss");
    Promise.resolve(onDiscussPlan(interactionId, content)).finally(() => {
      setSubmittingAction(null);
    });
  }

  function submitFreshImplementation() {
    if (stopping || submitting) return;
    setSubmittingAction("fresh");
    Promise.resolve(onImplementPlanFresh(interactionId)).finally(() => {
      setSubmittingAction(null);
    });
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
      if (!stopping && !submitting) {
        onCancel(interactionId);
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

    if (event.key === "Enter") {
      event.preventDefault();
      submitDiscussion();
    }
  }

  return (
    <div
      ref={composerRef}
      className="border-t border-border"
      onKeyDown={handleComposerKeyDown}
      tabIndex={-1}
    >
      <div className="text-sm font-semibold px-4 pt-3">{t("planConfirm.promptTitle")}</div>
      <div className="px-4 py-2">
        <textarea
          ref={textareaRef}
          value={message}
          onChange={(event) => setMessage(event.target.value)}
          onKeyDown={submitOnShortcut}
          placeholder={t("planConfirm.discussPlaceholder")}
          aria-label={t("planConfirm.discussPlaceholder")}
          disabled={stopping || submitting}
          className="flex min-h-[72px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
        />
      </div>
      <div className="flex items-center justify-between px-4 pb-3">
        <span className="flex items-center gap-1 text-xs text-muted-foreground">
          <kbd className="px-1 py-0.5 rounded border border-border bg-muted text-[10px] font-mono">Ctrl</kbd>
          <span>/</span>
          <kbd className="px-1 py-0.5 rounded border border-border bg-muted text-[10px] font-mono">⌘</kbd>
          <span>+</span>
          <kbd className="px-1 py-0.5 rounded border border-border bg-muted text-[10px] font-mono">Enter</kbd>
        </span>
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={submitFreshImplementation}
            disabled={stopping || submitting}
          >
            {submittingAction === "fresh" ? <Loader2 size={13} className="animate-spin" /> : null}
            <span>{t("planConfirm.implementFreshChoice")}</span>
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            onClick={() => onCancel(interactionId)}
            disabled={stopping || submitting}
          >
            <span>{t("planConfirm.ignore")}</span>
            <kbd className="px-1 py-0.5 rounded border border-border bg-muted text-[10px] font-mono ml-1">ESC</kbd>
          </Button>
          <Button
            type="button"
            size="sm"
            onClick={submitDiscussion}
            disabled={stopping || submitting || !message.trim()}
          >
            {submittingAction === "discuss" ? <Loader2 size={13} className="animate-spin" /> : null}
            <span>{t("planConfirm.continueDiscussion")}</span>
            <CornerDownLeft size={13} />
          </Button>
        </div>
      </div>
    </div>
  );
}

function ToolApprovalComposer({
  interaction,
  stopping,
  onResolve,
  t,
}: {
  interaction: InteractionRequest;
  stopping: boolean;
  onResolve: (interactionId: string, resolution: InteractionResolution) => void;
  t: TFunction;
}) {
  if (interaction.payload.type !== "toolApproval") {
    return null;
  }
  const args =
    typeof interaction.payload.arguments === "string"
      ? interaction.payload.arguments
      : JSON.stringify(interaction.payload.arguments, null, 2);
  return (
    <div className="border-t border-border px-4 py-3">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2 text-sm font-semibold">
            <Wrench size={15} />
            <span className="truncate">{interaction.payload.name}</span>
          </div>
          {interaction.payload.workingDirectory ? (
            <p className="text-xs text-muted-foreground truncate">{interaction.payload.workingDirectory}</p>
          ) : null}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <Button
            type="button"
            variant="destructive"
            size="sm"
            disabled={stopping}
            onClick={() =>
              onResolve(interaction.interactionId, {
                type: "toolApproval",
                decision: "denied",
                reason: "denied by user",
              })
            }
          >
            {t("actions.deny")}
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={stopping}
            onClick={() =>
              onResolve(interaction.interactionId, {
                type: "toolApproval",
                decision: "approved",
              })
            }
          >
            {t("actions.approve")}
          </Button>
        </div>
      </div>
      {args ? (
        <pre className="mt-2 max-h-24 overflow-auto rounded-md border border-border bg-muted/40 p-2 text-xs">
          {args}
        </pre>
      ) : null}
    </div>
  );
}

export function ConversationPanel({
  selectedSession,
  isBusy,
  entries,
  agents,
  sessionRuntime,
  lspServers,
  activeInteraction,
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
  onImplementPlanFresh,
  onSendPrompt,
  onDiscussPlan,
  onDismissPlanAction,
  onStopPrompt,
  onResolveInteraction,
}: ConversationPanelProps) {
  const { t } = useTranslation();
  const stopping = turnPhase === "stopping";
  const currentMode: CompileMode = selectedSession?.mode === "plan" ? "plan" : "auto";

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
    <section className="flex flex-1 min-w-0 flex-col h-screen bg-card overflow-hidden" data-status={status}>
      <header className="flex items-center gap-2 px-8 h-11 border-b border-border shrink-0">
        <h1 className="min-w-0 m-0 text-sm font-semibold truncate">
          {selectedSession?.title ?? t("conversation.defaultTitle")}
        </h1>
      </header>

      <ConversationTimeline
        sessionId={selectedSession?.id ?? null}
        entries={entries}
        isBusy={isBusy}
        scrollToBottomLabel={t("toolCall.scrollToBottom")}
        renderEntry={renderTimelineEntry}
        emptyState={
          <div className="flex flex-col items-center justify-center gap-2 text-muted-foreground py-12">
            <Terminal size={34} />
            <h2 className="text-lg font-semibold">{t("conversation.emptyTitle")}</h2>
            <p className="text-sm">{t("conversation.emptyDescription")}</p>
          </div>
        }
      />

      <footer className="border-t border-border">
        {activeInteraction?.payload.type === "userInput" ? (
          <AskUserComposer
            interactionId={activeInteraction.interactionId}
            questions={activeInteraction.payload.questions}
            stopping={stopping}
            onAnswer={(interactionId, response) =>
              onResolveInteraction(interactionId, {
                type: "userInput",
                answers: response.answers,
              })
            }
            t={t}
          />
        ) : activeInteraction?.payload.type === "toolApproval" ? (
          <ToolApprovalComposer
            interaction={activeInteraction}
            stopping={stopping}
            onResolve={onResolveInteraction}
            t={t}
          />
        ) : activeInteraction?.payload.type === "planConfirmation" ? (
          <PlanConfirmComposer
            interactionId={activeInteraction.interactionId}
            stopping={stopping}
            onImplementPlanFresh={onImplementPlanFresh}
            onDiscussPlan={onDiscussPlan}
            onCancel={onDismissPlanAction}
            t={t}
          />
        ) : (
          <div className="px-4 py-3">
            <div className="relative w-full">
              <Textarea
                value={prompt}
                onChange={(e) => onSetPrompt(e.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                    event.preventDefault();
                    onSendPrompt();
                  }
                }}
                placeholder={selectedSession ? t("conversation.askPlaceholder") : t("conversation.noSessionPlaceholder")}
                disabled={!selectedSession || isBusy}
                className="resize-none pr-12 min-h-[48px] max-h-[200px] rounded-xl px-4 py-3 bg-background"
                rows={1}
              />
              <Button
                size="icon"
                className="absolute right-2.5 bottom-2.5 h-8 w-8 rounded-lg shadow-sm"
                variant={isBusy ? "destructive" : "default"}
                disabled={isBusy ? stopping : !prompt.trim() || !selectedSession}
                onClick={isBusy ? onStopPrompt : onSendPrompt}
              >
                {isBusy ? <Square size={16} /> : <Send size={16} />}
              </Button>
            </div>
          </div>
        )}

        <SessionStatusBar
          runtime={sessionRuntime}
          lspServers={lspServers}
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
