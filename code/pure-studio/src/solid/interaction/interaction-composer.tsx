import { Check, ChevronLeft, ChevronRight, HelpCircle, Pause, ShieldAlert, X } from "lucide-solid";
import { For, Match, Show, Switch, createMemo, createSignal } from "solid-js";
import type {
  InteractionRequest,
  InteractionResolution,
  UserQuestion,
} from "../../types";
import i18n from "../../i18n";
import { MarkdownContent } from "../markdown";
import { DockPrompt } from "./dock-prompt";
import {
  buildPlanConfirmationResolution,
  buildToolApprovalResolution,
  buildUserInputResolution,
  prettyJson,
  type InteractionComposerState,
  type PlanConfirmationDecision,
  type ToolApprovalDecision,
  type UserInputDraft,
} from "./interaction-resolution";

export function InteractionComposer(props: {
  interaction: InteractionRequest;
  state: InteractionComposerState;
  error?: string | null;
  onResolve: (interaction: InteractionRequest, resolution: InteractionResolution) => void | Promise<void>;
}) {
  const disabled = () => props.state === "responding";

  function resolve(resolution: InteractionResolution) {
    if (disabled()) return;
    void props.onResolve(props.interaction, resolution);
  }

  return (
    <div class="interaction-composer" data-kind={props.interaction.kind}>
      <Switch>
        <Match when={props.interaction.payload.type === "userInput" ? props.interaction.payload : null}>
          {(payload) => (
            <UserInputComposer
              questions={payload().questions}
              disabled={disabled()}
              state={props.state}
              error={props.error}
              onSubmit={(draft) => resolve(buildUserInputResolution(payload().questions, draft))}
            />
          )}
        </Match>
        <Match when={props.interaction.payload.type === "toolApproval" ? props.interaction.payload : null}>
          {(payload) => (
            <ToolApprovalComposer
              name={payload().name}
              args={payload().arguments}
              workingDirectory={payload().workingDirectory}
              parentAgentId={payload().parentAgentId}
              agentPath={props.interaction.scope.agentPath}
              disabled={disabled()}
              state={props.state}
              error={props.error}
              onSubmit={(decision, reason) => resolve(buildToolApprovalResolution(decision, reason))}
            />
          )}
        </Match>
        <Match when={props.interaction.payload.type === "planConfirmation" ? props.interaction.payload : null}>
          {(payload) => (
            <PlanConfirmationComposer
              content={payload().content}
              disabled={disabled()}
              state={props.state}
              error={props.error}
              onSubmit={(decision, content, reason) => resolve(buildPlanConfirmationResolution(decision, content, reason))}
            />
          )}
        </Match>
      </Switch>
    </div>
  );
}

function UserInputComposer(props: {
  questions: UserQuestion[];
  disabled: boolean;
  state: InteractionComposerState;
  error?: string | null;
  onSubmit: (draft: UserInputDraft) => void;
}) {
  const [draft, setDraft] = createSignal<UserInputDraft>(initialDraft(props.questions));
  const [tab, setTab] = createSignal(0);
  const current = createMemo(() => props.questions[Math.max(0, Math.min(tab(), props.questions.length - 1))]);
  const last = createMemo(() => tab() >= props.questions.length - 1);
  const total = createMemo(() => props.questions.length);

  function toggle(question: UserQuestion, answer: string, checked: boolean) {
    setDraft((current) => {
      const selected = new Set(current[question.id]?.selected ?? []);
      if (checked) selected.add(answer);
      else selected.delete(answer);
      return {
        ...current,
        [question.id]: {
          ...current[question.id],
          selected: [...selected],
        },
      };
    });
  }

  function setFreeText(question: UserQuestion, value: string) {
    setDraft((current) => ({
      ...current,
      [question.id]: {
        ...current[question.id],
        freeText: value,
      },
    }));
  }

  function answered(question: UserQuestion) {
    const item = draft()[question.id];
    return Boolean((item?.selected?.length ?? 0) > 0 || item?.freeText?.trim());
  }

  function next() {
    if (last()) {
      props.onSubmit(draft());
      return;
    }
    setTab((value) => Math.min(total() - 1, value + 1));
  }

  function previous() {
    setTab((value) => Math.max(0, value - 1));
  }

  return (
    <DockPrompt
      kind="question"
      header={
        <>
          <div class="dock-title">
            <HelpCircle size={16} />
            <span>{i18n.t("askUser.questionCount", { count: total() })}</span>
          </div>
          <div class="dock-progress">
            <For each={props.questions}>
              {(question, index) => (
                <button
                  type="button"
                  data-active={index() === tab() || undefined}
                  data-answered={answered(question) || undefined}
                  disabled={props.disabled}
                  onClick={() => setTab(index())}
                  aria-label={i18n.t("askUser.questionLabel", { index: index() + 1 })}
                />
              )}
            </For>
          </div>
        </>
      }
      footer={
        <>
          <Show when={props.error}>
            {(error) => <span class="dock-error" role="alert">{error()}</span>}
          </Show>
          <div class="dock-actions">
            <Show when={tab() > 0}>
              <button type="button" class="secondary" disabled={props.disabled} onClick={previous}>
                <ChevronLeft size={14} />
                {i18n.t("actions.back")}
              </button>
            </Show>
            <button type="button" disabled={props.disabled} onClick={next}>
              <Show when={last()} fallback={<ChevronRight size={14} />}>
                <Check size={14} />
              </Show>
              {last() ? (props.state === "responding" ? i18n.t("askUser.submitting") : i18n.t("askUser.submit")) : i18n.t("askUser.next")}
            </button>
          </div>
        </>
      }
    >
      <Show when={current()}>
        {(question) => (
          <section class="interaction-question">
            <div class="interaction-question-title">
              <strong>{question().header || i18n.t("askUser.questionLabel", { index: tab() + 1 })}</strong>
              <span>{question().question}</span>
            </div>
            <Show when={question().options?.length}>
              <div class="interaction-options">
                <For each={question().options ?? []}>
                  {(option) => (
                    <label class="interaction-option">
                      <input
                        type="checkbox"
                        checked={draft()[question().id]?.selected?.includes(option.label) ?? false}
                        disabled={props.disabled}
                        onChange={(event) => toggle(question(), option.label, event.currentTarget.checked)}
                      />
                      <span>
                        <strong>{option.label}</strong>
                        <Show when={option.description}>
                          <small>{option.description}</small>
                        </Show>
                      </span>
                    </label>
                  )}
                </For>
              </div>
            </Show>
            <Show when={question().isOther || !question().options?.length}>
              <Show
                when={question().isSecret}
                fallback={
                  <textarea
                    value={draft()[question().id]?.freeText ?? ""}
                    disabled={props.disabled}
                    placeholder={i18n.t("askUser.answerPlaceholder")}
                    onInput={(event) => setFreeText(question(), event.currentTarget.value)}
                  />
                }
              >
                <input
                  type="password"
                  value={draft()[question().id]?.freeText ?? ""}
                  disabled={props.disabled}
                  placeholder={i18n.t("askUser.secretPlaceholder")}
                  autocomplete="off"
                  onInput={(event) => setFreeText(question(), event.currentTarget.value)}
                />
              </Show>
            </Show>
          </section>
        )}
      </Show>
    </DockPrompt>
  );
}

function ToolApprovalComposer(props: {
  name: string;
  args: unknown;
  workingDirectory?: string | null;
  parentAgentId?: string | null;
  agentPath?: string | null;
  disabled: boolean;
  state: InteractionComposerState;
  error?: string | null;
  onSubmit: (decision: ToolApprovalDecision, reason: string) => void;
}) {
  const [reason, setReason] = createSignal("");
  const details = createMemo(() => [
    [i18n.t("approval.tool"), props.name],
    [i18n.t("approval.workingDirectory"), props.workingDirectory ?? i18n.t("common.notAvailable")],
    [i18n.t("approval.parentAgent"), props.parentAgentId ?? i18n.t("common.notAvailable")],
    [i18n.t("approval.agentPath"), props.agentPath ?? i18n.t("common.notAvailable")],
  ]);

  return (
    <DockPrompt
      kind="permission"
      header={
        <div class="dock-title">
          <ShieldAlert size={16} />
          <span>{i18n.t("approval.permissionRequired")}</span>
        </div>
      }
      footer={
        <>
          <Show when={props.error}>
            {(error) => <span class="dock-error" role="alert">{error()}</span>}
          </Show>
          <div class="dock-actions">
            <button type="button" class="secondary" disabled={props.disabled} onClick={() => props.onSubmit("denied", reason())}>
              <X size={14} />
              {i18n.t("actions.deny")}
            </button>
            <button type="button" disabled={props.disabled} onClick={() => props.onSubmit("approved", reason())}>
              <Check size={14} />
              {props.state === "responding" ? i18n.t("approval.approving") : i18n.t("approval.allow")}
            </button>
          </div>
        </>
      }
    >
      <div class="interaction-meta">
        <For each={details()}>
          {([label, value]) => (
            <div class="status-line">
              <span>{label}</span>
              <strong>{value}</strong>
            </div>
          )}
        </For>
      </div>
      <pre class="interaction-code"><code>{prettyJson(props.args)}</code></pre>
      <textarea
        value={reason()}
        disabled={props.disabled}
        placeholder={i18n.t("approval.reasonPlaceholder")}
        onInput={(event) => setReason(event.currentTarget.value)}
      />
    </DockPrompt>
  );
}

function PlanConfirmationComposer(props: {
  content: string;
  disabled: boolean;
  state: InteractionComposerState;
  error?: string | null;
  onSubmit: (decision: PlanConfirmationDecision, content: string, reason: string) => void;
}) {
  const [content, setContent] = createSignal("");
  const [reason, setReason] = createSignal("");

  return (
    <DockPrompt
      kind="plan"
      header={
        <div class="dock-title">
          <Check size={16} />
          <span>{i18n.t("planConfirm.promptTitle")}</span>
        </div>
      }
      footer={
        <>
          <Show when={props.error}>
            {(error) => <span class="dock-error" role="alert">{error()}</span>}
          </Show>
          <div class="dock-actions">
            <button
              type="button"
              class="secondary"
              disabled={props.disabled}
              onClick={() => props.onSubmit("dismiss", "", reason())}
            >
              <X size={14} />
              {i18n.t("planConfirm.ignore")}
            </button>
            <button
              type="button"
              class="secondary"
              disabled={props.disabled || !content().trim()}
              onClick={() => props.onSubmit("continuePlanning", content(), reason())}
            >
              <Pause size={14} />
              {i18n.t("planConfirm.adjustChoice")}
            </button>
            <button
              type="button"
              disabled={props.disabled}
              onClick={() => props.onSubmit("implementFreshContext", "", reason())}
            >
              <Check size={14} />
              {props.state === "responding" ? i18n.t("planConfirm.starting") : i18n.t("planConfirm.implementFreshChoice")}
            </button>
          </div>
        </>
      }
    >
      <Show when={props.content}>
        <div class="interaction-plan-preview">
          <MarkdownContent content={props.content} />
        </div>
      </Show>
      <textarea
        value={content()}
        disabled={props.disabled}
        placeholder={i18n.t("planConfirm.discussPlaceholder")}
        onInput={(event) => setContent(event.currentTarget.value)}
      />
      <textarea
        value={reason()}
        disabled={props.disabled}
        placeholder={i18n.t("approval.reasonPlaceholder")}
        onInput={(event) => setReason(event.currentTarget.value)}
      />
    </DockPrompt>
  );
}

function initialDraft(questions: UserQuestion[]): UserInputDraft {
  const draft: UserInputDraft = {};
  for (const question of questions) {
    draft[question.id] = { selected: [], freeText: "" };
  }
  return draft;
}
