import { Settings, SlidersHorizontal, Square } from "lucide-solid";
import { Tooltip } from "@ark-ui/solid";
import i18n from "../../i18n";

export function ConversationHeader(props: {
  title: string;
  status: string;
  onOpenSettings: () => void;
}) {
  return (
    <header class="conversation-header">
      <div class="conversation-title-block">
        <h1>{props.title}</h1>
        <p>{props.status}</p>
      </div>
      <div class="header-actions">
        <Tooltip.Root openDelay={350} closeDelay={80}>
          <Tooltip.Trigger asChild={(triggerProps) => (
            <button {...triggerProps()} type="button" class="icon-action" onClick={props.onOpenSettings} aria-label={i18n.t("nav.settings")}>
              <Settings size={16} />
            </button>
          )} />
          <Tooltip.Positioner>
            <Tooltip.Content class="ark-tooltip-content">{i18n.t("nav.settings")}</Tooltip.Content>
          </Tooltip.Positioner>
        </Tooltip.Root>
        <button type="button" class="icon-action" aria-label={i18n.t("statusBar.capabilities")}>
          <SlidersHorizontal size={16} />
        </button>
        <button type="button" class="icon-action" aria-label={i18n.t("actions.close")}>
          <Square size={15} />
        </button>
      </div>
    </header>
  );
}
