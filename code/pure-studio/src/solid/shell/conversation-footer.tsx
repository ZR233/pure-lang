import { Pause, Play, Plus } from "lucide-solid";
import { Show } from "solid-js";
import type { InteractionRequest, InteractionResolution } from "../../types";
import i18n from "../../i18n";
import { InteractionComposer } from "../interaction/interaction-composer";
import type { InteractionComposerState } from "../interaction/interaction-resolution";
import { ComposerPermissionSelect } from "./composer-permission-select";
import type { PermissionMode } from "../../types";

export function ConversationFooter(props: {
  prompt: string;
  busy: boolean;
  activeInteraction: InteractionRequest | null;
  resolvingInteractionId: string | null;
  interactionError: string | null;
  permissionMode: PermissionMode;
  onSetPrompt: (value: string) => void;
  onSavePermissionMode: (mode: PermissionMode) => void;
  onSubmit: () => void;
  onStop: () => void;
  onResolve: (interaction: InteractionRequest, resolution: InteractionResolution) => void;
}) {
  let textArea: HTMLTextAreaElement | undefined;
  const interactionState = (interaction: InteractionRequest): InteractionComposerState =>
    props.resolvingInteractionId === interaction.interactionId
      ? "responding"
      : props.interactionError
        ? "error"
        : "pending";

  return (
    <footer class="conversation-footer">
      <Show when={props.activeInteraction} fallback={
        <div class="composer">
          <textarea
            ref={textArea}
            value={props.prompt}
            placeholder={i18n.t("conversation.askPlaceholder")}
            onInput={(event) => props.onSetPrompt(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                props.onSubmit();
              }
            }}
          />
          <div class="composer-actions">
            <button type="button" class="composer-tool-button" aria-label={i18n.t("actions.add")}>
              <Plus size={16} />
            </button>
            <ComposerPermissionSelect
              value={props.permissionMode}
              disabled={props.busy}
              onChange={props.onSavePermissionMode}
            />
            <button type="button" class="send-button" onClick={props.busy ? props.onStop : props.onSubmit} aria-label={props.busy ? i18n.t("actions.stop") : i18n.t("actions.send")}>
              <Show when={props.busy} fallback={<Play size={15} />}>
                <Pause size={15} />
              </Show>
            </button>
          </div>
        </div>
      }>
        {(interaction) => (
          <div class="interaction-footer-shell">
            <InteractionComposer
              interaction={interaction()}
              state={interactionState(interaction())}
              error={props.interactionError}
              onResolve={props.onResolve}
            />
            <Show when={props.busy}>
              <button type="button" class="send-button interaction-stop-button" onClick={props.onStop} aria-label={i18n.t("actions.stop")}>
                <Pause size={15} />
              </button>
            </Show>
          </div>
        )}
      </Show>
    </footer>
  );
}
