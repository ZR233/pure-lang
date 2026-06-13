import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { ModelCapabilities, ModelRecord, ProviderRecord } from "../types";

type ProviderModelEditorProps = {
  provider: ProviderRecord;
  disabled?: boolean;
  onAddCustomModel: () => void;
  onUpdateCustomModel: (index: number, patch: Partial<ModelRecord>) => void;
  onRemoveCustomModel: (index: number) => void;
};

function modelEffortsText(model: ModelRecord) {
  return model.reasoningEfforts.join(", ");
}

function effortsFromText(value: string) {
  return value
    .split(",")
    .map((effort) => effort.trim())
    .filter(Boolean);
}

function notConfigured(t: (key: string) => string) {
  return t("provider.notConfigured");
}

function formatTokens(value: number | undefined | null, t: (key: string) => string) {
  if (value === undefined || value === null) {
    return notConfigured(t);
  }
  if (value >= 1_000_000) {
    return `${trimNumber(value / 1_000_000)}M`;
  }
  if (value >= 1_000) {
    return `${trimNumber(value / 1_000)}K`;
  }
  return value.toString();
}

function trimNumber(value: number) {
  return Number.isInteger(value) ? value.toString() : value.toFixed(1);
}

function formatList(values: string[] | undefined, t: (key: string) => string) {
  return values && values.length > 0 ? values.join(", ") : notConfigured(t);
}

function capabilityList(capabilities: ModelCapabilities): string[] {
  const values = [
    capabilities.streaming ? "streaming" : null,
    capabilities.temperature ? "temperature" : null,
    capabilities.reasoning ? "reasoning" : null,
    capabilities.webSearch ? "web_search" : null,
    capabilities.tools.functionCalling ? "function_calling" : null,
    capabilities.tools.parallelToolCalls ? "parallel_tool_calls" : null,
    capabilities.tools.customTools ? "custom_tools" : null,
    capabilities.tools.freeformTools ? "freeform_tools" : null,
  ];
  return values.filter((value): value is string => Boolean(value));
}

function formatPrice(model: ModelRecord, t: (key: string) => string) {
  if (
    !model.currency ||
    model.inputPricePerMTok == null ||
    model.outputPricePerMTok == null ||
    model.cacheReadPricePerMTok == null
  ) {
    return notConfigured(t);
  }
  return `${model.currency} ${model.cacheReadPricePerMTok}/${model.inputPricePerMTok}/${model.outputPricePerMTok}`;
}

function ModelParameterGrid({ model, t }: { model: ModelRecord; t: (key: string) => string }) {
  return (
    <div className="grid grid-cols-4 gap-2 mt-2 text-xs">
      <div>
        <small className="text-muted-foreground">{t("model.context")}</small>
        <p className="font-medium text-foreground">{formatTokens(model.contextWindow ?? model.maxContextWindow, t)}</p>
      </div>
      <div>
        <small className="text-muted-foreground">{t("model.maxOutput")}</small>
        <p className="font-medium text-foreground">{formatTokens(model.maxOutputTokens, t)}</p>
      </div>
      <div>
        <small className="text-muted-foreground">{t("model.autoCompact")}</small>
        <p className="font-medium text-foreground">{formatTokens(model.autoCompactTokenLimit, t)}</p>
      </div>
      <div>
        <small className="text-muted-foreground">{t("model.temperature")}</small>
        <p className="font-medium text-foreground">{model.defaultTemperature ?? notConfigured(t)}</p>
      </div>
      <div>
        <small className="text-muted-foreground">{t("model.efforts")}</small>
        <p className="font-medium text-foreground">{modelEffortsText(model) || notConfigured(t)}</p>
      </div>
      <div>
        <small className="text-muted-foreground">{t("model.truncation")}</small>
        <p className="font-medium text-foreground">
          {model.truncationMode ?? "tokens"} / {formatTokens(model.truncationLimit, t)}
        </p>
      </div>
      <div className="col-span-2">
        <small className="text-muted-foreground">{t("model.pricing")}</small>
        <p className="font-medium text-foreground">{formatPrice(model, t)}</p>
      </div>
      <div className="col-span-2">
        <small className="text-muted-foreground">{t("model.capabilities")}</small>
        <p className="font-medium text-foreground">{formatList(capabilityList(model.capabilities), t)}</p>
      </div>
      <div className="col-span-2">
        <small className="text-muted-foreground">{t("model.input")}</small>
        <p className="font-medium text-foreground">{formatList(model.capabilities.input, t)}</p>
      </div>
    </div>
  );
}

export function ProviderModelEditor({
  provider,
  disabled = false,
  onAddCustomModel,
  onUpdateCustomModel,
  onRemoveCustomModel,
}: ProviderModelEditorProps) {
  const { t } = useTranslation();
  return (
    <div className="space-y-4 mt-4">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h3 className="text-base font-semibold text-foreground">{t("model.title")}</h3>
          <p className="text-sm text-muted-foreground">{t("model.defaultModelDesc")}</p>
        </div>
        <Button variant="outline" size="sm" disabled={disabled} onClick={onAddCustomModel}>
          <Plus size={16} className="mr-1" />
          {t("model.customModelButton")}
        </Button>
      </div>

      <div className="text-sm font-medium text-muted-foreground">{t("model.defaultModels")}</div>
      <div className="grid gap-3">
        {provider.defaultModels.map((model) => (
          <Card className="p-3" key={model.slug}>
            <div className="flex items-center gap-2">
              <strong className="text-sm text-foreground">{model.displayName}</strong>
              <span className="text-xs text-muted-foreground">{model.slug}</span>
            </div>
            {model.description ? <p className="text-xs text-muted-foreground mt-1">{model.description}</p> : null}
            <ModelParameterGrid model={model} t={t} />
          </Card>
        ))}
      </div>

      <div className="text-sm font-medium text-muted-foreground">{t("model.customModels")}</div>
      <div className="grid gap-3">
        {provider.customModels.length === 0 ? (
          <p className="text-sm text-muted-foreground">{t("model.noCustomModels")}</p>
        ) : (
          provider.customModels.map((model, index) => (
            <Card className="p-3" key={`${model.slug}-${index}`}>
              <div className="flex items-center gap-2">
                <Input
                  disabled={disabled}
                  value={model.slug}
                  onChange={(event) => onUpdateCustomModel(index, { slug: event.target.value })}
                  placeholder={t("model.slugPlaceholder")}
                  className="flex-1"
                />
                <Input
                  disabled={disabled}
                  value={model.displayName}
                  onChange={(event) =>
                    onUpdateCustomModel(index, { displayName: event.target.value })
                  }
                  placeholder={t("model.displayNamePlaceholder")}
                  className="flex-1"
                />
                <Input
                  disabled={disabled}
                  value={modelEffortsText(model)}
                  onChange={(event) =>
                    onUpdateCustomModel(index, {
                      reasoningEfforts: effortsFromText(event.target.value),
                    })
                  }
                  placeholder={t("model.effortsPlaceholder")}
                  className="flex-1"
                />
                <Button
                  variant="ghost"
                  size="icon"
                  disabled={disabled}
                  onClick={() => onRemoveCustomModel(index)}
                  title={t("model.deleteTooltip")}
                >
                  <Trash2 size={16} />
                </Button>
              </div>
              <ModelParameterGrid model={model} t={t} />
            </Card>
          ))
        )}
      </div>
    </div>
  );
}
