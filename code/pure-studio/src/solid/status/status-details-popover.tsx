import { HoverCard } from "@ark-ui/solid";
import { Show, type JSX } from "solid-js";

export function StatusDetailsPopover(props: {
  icon?: JSX.Element;
  label?: string;
  children: JSX.Element;
  class?: string;
}) {
  return (
    <HoverCard.Root openDelay={80} closeDelay={80} positioning={{ placement: "top-end", gutter: 8 }}>
      <div class={`status-details ${props.class ?? ""}`}>
        <HoverCard.Trigger class="status-summary">
          {props.icon}
          <Show when={props.label}>
            {(label) => <span>{label()}</span>}
          </Show>
        </HoverCard.Trigger>
        <HoverCard.Positioner>
          <HoverCard.Content class="status-popover">
            {props.children}
          </HoverCard.Content>
        </HoverCard.Positioner>
      </div>
    </HoverCard.Root>
  );
}
