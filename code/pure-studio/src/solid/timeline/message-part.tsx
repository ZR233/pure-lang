// SPDX-License-Identifier: MIT
// Adapted from opencode packages/ui/src/components/message-part.tsx.

import { ChevronDown, CircleAlert, FileText, Hammer, HelpCircle, Loader2, TerminalSquare } from "lucide-solid";
import { For, Show, createMemo, createSignal } from "solid-js";
import type { StudioPart, UserQuestion } from "../../types";
import i18n from "../../i18n";
import { MarkdownContent } from "../markdown";
import { readPartText, readPlanContent, readToolArguments, readToolResult, reasoningHeading } from "./message-part-text";
import {
  groupParts,
  isActiveStatus,
  isQuestionTool,
  isTerminalProblem,
  isTerminalStatus,
  partDefaultOpen,
  reasoningDefaultOpen,
  renderable,
  renderableWithDelta,
  sameGroups,
  type PartGroup,
  type PartRef,
} from "./message-part.data";

export {
  groupParts,
  partDefaultOpen,
  reasoningDefaultOpen,
  renderable,
  renderableWithDelta,
  sameGroups,
  type PartGroup,
  type PartRef,
};

export function MessagePart(props: {
  part: StudioPart;
  deltaText?: string;
  working?: boolean;
  defaultOpen?: boolean;
}) {
  const text = () => readPartText(props.deltaText, props.part);
  const live = () => isActiveStatus(props.part.status) || props.deltaText !== undefined;
  return (
    <div class="oc-part" data-part-type={props.part.partType} data-part-status={props.part.status}>
      <Show when={props.part.partType === "text"}>
        <TextPart part={props.part} text={text()} live={live()} />
      </Show>
      <Show when={props.part.partType === "reasoning"}>
        <ReasoningPart part={props.part} text={text()} defaultOpen={props.defaultOpen} />
      </Show>
      <Show when={props.part.partType === "plan"}>
        <PlanPart part={props.part} deltaText={props.deltaText} live={live()} />
      </Show>
      <Show when={props.part.partType === "tool"}>
        <ToolPart part={props.part} deltaText={props.deltaText} defaultOpen={props.defaultOpen} />
      </Show>
      <Show when={props.part.partType === "agent"}>
        <AgentPart part={props.part} />
      </Show>
      <Show when={props.part.partType === "turn" && isTerminalStatus(props.part.status)}>
        <NoticePart part={props.part} />
      </Show>
    </div>
  );
}

export function ContextToolGroup(props: { parts: StudioPart[] }) {
  return (
    <div class="oc-context-group">
      <div class="oc-context-group-header">
        <FileText size={14} />
        <span>{i18n.t("toolGroup.contextItems", { count: props.parts.length })}</span>
      </div>
      <div class="oc-context-group-list">
        <For each={props.parts}>
          {(part) => <span class="oc-context-chip">{toolPathSummary(part)}</span>}
        </For>
      </div>
    </div>
  );
}

function TextPart(props: { part: StudioPart; text: string; live?: boolean }) {
  const channel = props.part.textChannel;
  if (channel === "commentary") {
    return <div class="oc-commentary"><MarkdownContent content={props.text} live={props.live} /></div>;
  }
  return <MarkdownContent content={props.text} live={props.live} />;
}

function ReasoningPart(props: { part: StudioPart; text: string; defaultOpen?: boolean }) {
  const active = isActiveStatus(props.part.status);
  const heading = () => reasoningHeading(props.text) ?? (active ? i18n.t("timeline.thinkingActive") : thoughtDuration(props.part));
  return (
    <details
      class="oc-reasoning"
      open={reasoningDefaultOpen(props.part, props.defaultOpen) ? true : undefined}
      data-default-open={props.defaultOpen ? "true" : "false"}
      data-active={active || undefined}
    >
      <summary>
        <span class="oc-reasoning-icon">
          <Show when={active} fallback={<FileText size={14} />}>
            <Loader2 size={14} class="spin" />
          </Show>
        </span>
        <span>{heading()}</span>
        <ChevronDown size={14} class="oc-chevron" />
      </summary>
      <Show when={!active}>
        <pre>{props.text}</pre>
      </Show>
    </details>
  );
}

function PlanPart(props: { part: StudioPart; deltaText?: string; live?: boolean }) {
  const content = () => readPlanContent(props.deltaText, props.part);
  return (
    <div class="oc-plan">
      <div class="oc-plan-header">
        <FileText size={15} />
        <span>{i18n.t("timeline.plan")}</span>
      </div>
      <MarkdownContent content={content()} live={props.live} />
    </div>
  );
}

function ToolPart(props: { part: StudioPart; deltaText?: string; defaultOpen?: boolean }) {
  if (isQuestionTool(props.part)) return <QuestionToolPart part={props.part} />;
  const [open, setOpen] = createSignal(props.defaultOpen ?? (isActiveStatus(props.part.status) || isTerminalProblem(props.part.status)));
  const tool = () => props.part.tool;
  const args = () => readToolArguments(props.deltaText, props.part);
  const result = () => readToolResult(props.deltaText, props.part);
  return (
    <div class="oc-tool" data-open={open() || undefined}>
      <button type="button" class="oc-tool-header" onClick={() => setOpen(!open())}>
        <span class="oc-tool-icon">
          <Show when={isActiveStatus(props.part.status)} fallback={<Hammer size={14} />}>
            <Loader2 size={14} class="spin" />
          </Show>
        </span>
        <span class="oc-tool-name">{tool()?.name || i18n.t("toolGroup.title")}</span>
        <span class="oc-tool-summary">{toolPathSummary(props.part)}</span>
        <span class="oc-tool-status">{statusLabel(props.part.status)}</span>
        <ChevronDown size={14} class="oc-chevron" />
      </button>
      <Show when={open()}>
        <div class="oc-tool-body">
          <Show when={args()}>
            <pre>{args()}</pre>
          </Show>
          <Show when={result()}>
            <pre>{result()}</pre>
          </Show>
          <Show when={props.part.error}>
            <div class="oc-error"><CircleAlert size={14} />{props.part.error}</div>
          </Show>
        </div>
      </Show>
    </div>
  );
}

function QuestionToolPart(props: { part: StudioPart }) {
  const questions = createMemo(() => readQuestionToolInput(props.part));
  const answers = createMemo(() => readQuestionToolAnswers(props.part));
  const answeredCount = createMemo(() => questions().filter((question) => (answers()[question.id]?.answers.length ?? 0) > 0).length);
  const [open, setOpen] = createSignal(props.part.status === "completed");
  return (
    <div class="oc-tool oc-question-tool" data-open={open() || undefined}>
      <button type="button" class="oc-tool-header" onClick={() => setOpen(!open())}>
        <span class="oc-tool-icon"><HelpCircle size={14} /></span>
        <span class="oc-tool-name">{i18n.t("askUser.questions")}</span>
        <span class="oc-tool-summary">{i18n.t("askUser.answeredCount", { count: answeredCount() })}</span>
        <span class="oc-tool-status">{statusLabel(props.part.status)}</span>
        <ChevronDown size={14} class="oc-chevron" />
      </button>
      <Show when={open()}>
        <div class="oc-question-answers">
          <For each={questions()}>
            {(question) => {
              const answer = () => answers()[question.id]?.answers ?? [];
              return (
                <div class="oc-question-answer-item">
                  <div class="oc-question-text">{question.question}</div>
                  <div class="oc-answer-text">{answer().join(", ") || i18n.t("askUser.noAnswer")}</div>
                </div>
              );
            }}
          </For>
          <Show when={questions().length === 0 && props.part.tool?.result}>
            <pre>{props.part.tool?.result}</pre>
          </Show>
        </div>
      </Show>
    </div>
  );
}

function AgentPart(props: { part: StudioPart }) {
  const agent = () => props.part.agent;
  const summary = () => agent()?.summary ?? agent()?.error;
  return (
    <div class="oc-agent">
      <div class="oc-agent-header">
        <TerminalSquare size={14} />
        <span>{agent()?.path ?? i18n.t("subagent.title")}</span>
        <span class="oc-muted">{agent()?.status ?? props.part.status}</span>
      </div>
      <Show when={summary()}>
        {(value) => (
          <div class="oc-agent-summary">
            <MarkdownContent content={value()} />
          </div>
        )}
      </Show>
    </div>
  );
}

function NoticePart(props: { part: StudioPart }) {
  return <div class="oc-notice"><CircleAlert size={14} />{props.part.error || props.part.text || statusLabel(props.part.status)}</div>;
}

function statusLabel(status: StudioPart["status"]) {
  switch (status) {
    case "started":
    case "streaming":
      return i18n.t("toolCall.streaming");
    case "awaitingApproval":
      return i18n.t("toolCall.pendingApproval");
    case "approved":
      return i18n.t("toolCall.approved");
    case "running":
      return i18n.t("toolCall.streaming");
    case "completed":
      return i18n.t("toolCall.resultReady");
    case "failed":
      return i18n.t("subagent.failed");
    case "denied":
      return i18n.t("toolCall.denied");
    case "interrupted":
      return i18n.t("status.interrupted");
    case "budgetLimited":
      return i18n.t("turnPhase.budgetLimited");
  }
}

function thoughtDuration(part: StudioPart) {
  const seconds = Math.max(0, (part.updatedAt ?? part.createdAt) - part.createdAt);
  if (seconds <= 0) return i18n.t("timeline.thinking");
  return i18n.t("timeline.thoughtDurationSecondsShort", { seconds });
}

function toolPathSummary(part: StudioPart) {
  const args = part.tool?.arguments;
  if (!args) return "";
  try {
    const parsed = JSON.parse(args) as Record<string, unknown>;
    const value = parsed.path ?? parsed.filePath ?? parsed.file_path ?? parsed.command ?? parsed.input;
    if (typeof value === "string") return value;
    if (Array.isArray(value)) return value.filter((item): item is string => typeof item === "string").slice(0, 3).join(", ");
  } catch {
    return args.length > 80 ? `${args.slice(0, 77)}...` : args;
  }
  return "";
}

function readQuestionToolInput(part: StudioPart): UserQuestion[] {
  const value = parseJson(part.tool?.arguments);
  if (!record(value)) return [];
  const questions = Array.isArray(value.questions) ? value.questions : [];
  return questions.flatMap((question, index) => {
    if (!record(question)) return [];
    const text = typeof question.question === "string" ? question.question : "";
    if (!text) return [];
    const id = typeof question.id === "string" && question.id ? question.id : `question-${index}`;
    const header = typeof question.header === "string" ? question.header : "";
    const isOther = typeof question.isOther === "boolean"
      ? question.isOther
      : typeof question.is_other === "boolean"
        ? question.is_other
        : undefined;
    const isSecret = typeof question.isSecret === "boolean"
      ? question.isSecret
      : typeof question.is_secret === "boolean"
        ? question.is_secret
        : undefined;
    const options = Array.isArray(question.options)
      ? question.options.flatMap((option) => {
          if (typeof option === "string") return [{ label: option, description: "" }];
          if (!record(option) || typeof option.label !== "string") return [];
          return [{
            label: option.label,
            description: typeof option.description === "string" ? option.description : "",
          }];
        })
      : null;
    return [{ id, header, question: text, isOther, isSecret, options }];
  });
}

function readQuestionToolAnswers(part: StudioPart): Record<string, { answers: string[] }> {
  const value = parseJson(part.tool?.result);
  if (!record(value) || !record(value.answers)) return {};
  const result: Record<string, { answers: string[] }> = {};
  for (const [id, answer] of Object.entries(value.answers)) {
    if (!record(answer) || !Array.isArray(answer.answers)) continue;
    result[id] = {
      answers: answer.answers.filter((item): item is string => typeof item === "string"),
    };
  }
  return result;
}

function parseJson(value: string | null | undefined) {
  if (!value) return undefined;
  try {
    return JSON.parse(value) as unknown;
  } catch {
    return undefined;
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
