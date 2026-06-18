import { Check, Pencil, X } from "lucide-solid";
import { Show, createSignal } from "solid-js";
import i18n from "../../i18n";
import { DockShell } from "./dock-shell";
import type { InteractionComposerState, PlanConfirmationDecision } from "./interaction-resolution";

export function PlanConfirmationDock(props: {
  content: string;
  disabled: boolean;
  state: InteractionComposerState;
  error?: string | null;
  onSubmit: (decision: PlanConfirmationDecision, content: string, reason: string) => void;
}) {
  const [adjustment, setAdjustment] = createSignal("");
  const [editing, setEditing] = createSignal(false);

  function submitAdjust() {
    const value = adjustment().trim();
    if (!value) return;
    props.onSubmit("continuePlanning", value, "");
  }

  return (
    <DockShell
      kind="plan"
      header={<h2 class="dock-plan-question">{i18n.t("planConfirm.implementQuestion")}</h2>}
      footer={
        <>
          <Show when={props.error}>
            {(error) => <span class="dock-error" role="alert">{error()}</span>}
          </Show>
          <div class="dock-actions plan-actions">
            <button
              type="button"
              class="dock-ignore"
              disabled={props.disabled}
              onClick={() => props.onSubmit("dismiss", "", "")}
            >
              <X size={13} />
              {i18n.t("planConfirm.ignore")}
              <span class="dock-kbd">ESC</span>
            </button>
            <button
              type="button"
              class="dock-button primary"
              disabled={props.disabled || (editing() && !adjustment().trim())}
              onClick={() => editing() ? submitAdjust() : props.onSubmit("implementFreshContext", "", "")}
            >
              <Check size={14} />
              {props.state === "responding" ? i18n.t("planConfirm.starting") : i18n.t("planConfirm.submit")}
            </button>
          </div>
        </>
      }
    >
      <div class="plan-choice-list">
        <button
          type="button"
          class="plan-choice primary-choice"
          disabled={props.disabled}
          onClick={() => props.onSubmit("implementFreshContext", "", "")}
        >
          <span class="plan-choice-index">1</span>
          <strong>{i18n.t("planConfirm.yesImplement")}</strong>
          <span class="plan-choice-shortcuts">↑ ↓</span>
        </button>
        <div class="plan-choice adjust-choice" data-editing={editing() || undefined}>
          <button
            type="button"
            class="adjust-choice-trigger"
            disabled={props.disabled}
            onClick={() => setEditing(true)}
          >
            <Pencil size={14} />
            <span>{i18n.t("planConfirm.noAdjust")}</span>
          </button>
          <Show when={editing()}>
            <textarea
              value={adjustment()}
              disabled={props.disabled}
              placeholder={i18n.t("planConfirm.adjustPlaceholder")}
              onInput={(event) => setAdjustment(event.currentTarget.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  submitAdjust();
                }
              }}
              autofocus
            />
          </Show>
        </div>
      </div>
    </DockShell>
  );
}
