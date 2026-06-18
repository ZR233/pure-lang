import { ChevronDown, FileText } from "lucide-solid";
import { Show, createMemo, createSignal } from "solid-js";
import type { StudioPart } from "../../types";
import i18n from "../../i18n";
import { MarkdownContent } from "../markdown";
import { readPlanContent } from "./message-part-text";

const collapsedPlanLength = 900;

export function PlanCard(props: { part: StudioPart; deltaText?: string; live?: boolean }) {
  const [expanded, setExpanded] = createSignal(false);
  const content = () => readPlanContent(props.deltaText, props.part);
  const collapsible = createMemo(() => content().length > collapsedPlanLength);
  const collapsed = createMemo(() => collapsible() && !expanded());

  return (
    <div class="oc-plan" data-collapsed={collapsed() || undefined}>
      <div class="oc-plan-header">
        <FileText size={15} />
        <span>{i18n.t("timeline.plan")}</span>
      </div>
      <div class="oc-plan-body">
        <MarkdownContent content={content()} live={props.live} />
        <Show when={collapsed()}>
          <div class="oc-plan-fade">
            <button type="button" class="oc-plan-expand" onClick={() => setExpanded(true)}>
              {i18n.t("timeline.expandPlan")}
            </button>
          </div>
        </Show>
      </div>
      <Show when={collapsible() && expanded()}>
        <button type="button" class="oc-plan-collapse" onClick={() => setExpanded(false)}>
          <ChevronDown size={14} />
          {i18n.t("timeline.collapsePlan")}
        </button>
      </Show>
    </div>
  );
}
