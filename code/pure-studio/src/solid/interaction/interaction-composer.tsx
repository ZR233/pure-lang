import { Match, Switch } from "solid-js";
import type {
  InteractionRequest,
  InteractionResolution,
} from "../../types";
import { PlanConfirmationDock } from "./plan-confirmation-dock";
import { ToolApprovalDock } from "./tool-approval-dock";
import { UserInputDock } from "./user-input-dock";
import {
  buildPlanConfirmationResolution,
  buildToolApprovalResolution,
  buildUserInputResolution,
  type InteractionComposerState,
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
            <UserInputDock
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
            <ToolApprovalDock
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
            <PlanConfirmationDock
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
