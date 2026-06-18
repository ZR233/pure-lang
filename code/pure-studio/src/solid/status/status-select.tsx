import { Check, ChevronDown } from "lucide-solid";
import { Select, createListCollection } from "@ark-ui/solid";
import { For, type JSX } from "solid-js";

export type StatusSelectOption = {
  value: string;
  label: string;
  disabled?: boolean;
};

export function StatusSelect(props: {
  value: string;
  options: StatusSelectOption[];
  onChange: (value: string) => void;
  disabled?: boolean;
  "aria-label": string;
  class?: string;
  icon?: JSX.Element;
}) {
  const collection = () => createListCollection({
    items: props.options,
    itemToString: (item) => item.label,
    itemToValue: (item) => item.value,
  });
  const selectedLabel = () => props.options.find((option) => option.value === props.value)?.label ?? props.value;

  return (
    <Select.Root
      collection={collection()}
      value={props.value ? [props.value] : []}
      disabled={props.disabled || props.options.length === 0}
      positioning={{ placement: "top-start", gutter: 8, sameWidth: true }}
      onValueChange={(details) => {
        const next = details.value[0];
        if (next !== undefined) props.onChange(next);
      }}
      class={`status-select ${props.class ?? ""}`}
    >
      <Select.Control>
        <Select.Trigger class="status-select-trigger" aria-label={props["aria-label"]}>
          {props.icon}
          <span class="status-select-value">{selectedLabel()}</span>
          <Select.Indicator class="status-select-indicator">
            <ChevronDown size={13} />
          </Select.Indicator>
        </Select.Trigger>
      </Select.Control>
      <Select.Positioner>
        <Select.Content class="status-select-content">
          <Select.List class="status-select-list">
            <For each={props.options}>
              {(option) => (
                <Select.Item item={option} class="status-select-item" data-disabled={option.disabled || undefined}>
                  <Select.ItemText class="status-select-item-text">{option.label}</Select.ItemText>
                  <Select.ItemIndicator class="status-select-item-indicator">
                    <Check size={14} />
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
