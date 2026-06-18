import type { JSX } from "solid-js";

export function DockShell(props: {
  kind: "question" | "permission" | "plan";
  header: JSX.Element;
  footer?: JSX.Element;
  children: JSX.Element;
  onKeyDown?: JSX.EventHandlerUnion<HTMLDivElement, KeyboardEvent>;
}) {
  return (
    <div class="dock-prompt" data-kind={props.kind} onKeyDown={props.onKeyDown}>
      <div class="dock-prompt-header">{props.header}</div>
      <div class="dock-prompt-body">{props.children}</div>
      <div class="dock-prompt-footer">{props.footer}</div>
    </div>
  );
}
