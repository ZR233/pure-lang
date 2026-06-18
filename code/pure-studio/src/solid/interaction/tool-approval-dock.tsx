import { Check, ShieldAlert, X } from "lucide-solid";
import { For, Show, createMemo, createSignal } from "solid-js";
import i18n from "../../i18n";
import { DockShell } from "./dock-shell";
import { prettyJson, type InteractionComposerState, type ToolApprovalDecision } from "./interaction-resolution";

export function ToolApprovalDock(props: {
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
    <DockShell
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
            <button type="button" class="dock-button secondary" disabled={props.disabled} onClick={() => props.onSubmit("denied", reason())}>
              <X size={14} />
              {i18n.t("actions.deny")}
            </button>
            <button type="button" class="dock-button primary" disabled={props.disabled} onClick={() => props.onSubmit("approved", reason())}>
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
    </DockShell>
  );
}
