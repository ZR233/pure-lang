import { Check, ChevronLeft, ChevronRight, HelpCircle } from "lucide-solid";
import { For, Show, createMemo, createSignal } from "solid-js";
import type { UserQuestion } from "../../types";
import i18n from "../../i18n";
import { DockShell } from "./dock-shell";
import type { InteractionComposerState, UserInputDraft } from "./interaction-resolution";

export function UserInputDock(props: {
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

  return (
    <DockShell
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
              <button type="button" class="dock-button secondary" disabled={props.disabled} onClick={() => setTab((value) => Math.max(0, value - 1))}>
                <ChevronLeft size={14} />
                {i18n.t("actions.back")}
              </button>
            </Show>
            <button type="button" class="dock-button primary" disabled={props.disabled} onClick={next}>
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
    </DockShell>
  );
}

function initialDraft(questions: UserQuestion[]): UserInputDraft {
  const draft: UserInputDraft = {};
  for (const question of questions) {
    draft[question.id] = { selected: [], freeText: "" };
  }
  return draft;
}
