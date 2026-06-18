import { Check, ChevronDown, ShieldCheck } from "lucide-solid";
import { Select, createListCollection } from "@ark-ui/solid";
import { For } from "solid-js";
import type { PermissionMode } from "../../types";
import i18n from "../../i18n";

const permissionModes: PermissionMode[] = ["request-approval", "auto-review", "full-access"];

export function ComposerPermissionSelect(props: {
  value: PermissionMode;
  disabled?: boolean;
  onChange: (mode: PermissionMode) => void;
}) {
  const options = () => permissionModes.map((mode) => ({
    value: mode,
    label: i18n.t(`permissionMode.${mode}`),
  }));
  const collection = () => createListCollection({
    items: options(),
    itemToString: (item) => item.label,
    itemToValue: (item) => item.value,
  });
  const label = () => options().find((option) => option.value === props.value)?.label ?? props.value;

  return (
    <Select.Root
      collection={collection()}
      value={[props.value]}
      disabled={props.disabled}
      positioning={{ placement: "top-start", gutter: 8, sameWidth: true }}
      onValueChange={(details) => {
        const next = details.value[0] as PermissionMode | undefined;
        if (next && next !== props.value) props.onChange(next);
      }}
      class="composer-permission-select"
    >
      <Select.Control>
        <Select.Trigger class="composer-permission-trigger" aria-label={i18n.t("statusBar.permissionMode")}>
          <ShieldCheck size={13} />
          <span>{label()}</span>
          <Select.Indicator class="composer-permission-indicator">
            <ChevronDown size={12} />
          </Select.Indicator>
        </Select.Trigger>
      </Select.Control>
      <Select.Positioner>
        <Select.Content class="composer-permission-content">
          <Select.List class="composer-permission-list">
            <For each={options()}>
              {(option) => (
                <Select.Item item={option} class="composer-permission-item">
                  <Select.ItemText class="composer-permission-item-text">{option.label}</Select.ItemText>
                  <Select.ItemIndicator class="composer-permission-item-indicator">
                    <Check size={13} />
                  </Select.ItemIndicator>
                </Select.Item>
              )}
            </For>
          </Select.List>
        </Select.Content>
      </Select.Positioner>
      <Select.HiddenSelect />
    </Select.Root>
  );
}
