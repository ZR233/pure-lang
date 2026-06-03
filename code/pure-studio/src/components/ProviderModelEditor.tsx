import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ModelRecord, ProviderRecord } from "../types";

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
    <div className="model-parameter-grid">
      <span>
        <small>{t("model.context")}</small>
        <strong>{formatTokens(model.contextWindow ?? model.maxContextWindow, t)}</strong>
      </span>
      <span>
        <small>{t("model.maxOutput")}</small>
        <strong>{formatTokens(model.maxOutputTokens, t)}</strong>
      </span>
      <span>
        <small>{t("model.autoCompact")}</small>
        <strong>{formatTokens(model.autoCompactTokenLimit, t)}</strong>
      </span>
      <span>
        <small>{t("model.temperature")}</small>
        <strong>{model.defaultTemperature ?? notConfigured(t)}</strong>
      </span>
      <span>
        <small>{t("model.efforts")}</small>
        <strong>{modelEffortsText(model) || notConfigured(t)}</strong>
      </span>
      <span>
        <small>{t("model.truncation")}</small>
        <strong>
          {model.truncationMode ?? "tokens"} / {formatTokens(model.truncationLimit, t)}
        </strong>
      </span>
      <span className="wide">
        <small>{t("model.pricing")}</small>
        <strong>{formatPrice(model, t)}</strong>
      </span>
      <span className="wide">
        <small>{t("model.capabilities")}</small>
        <strong>{formatList(model.capabilities, t)}</strong>
      </span>
      <span className="wide">
        <small>{t("model.input")}</small>
        <strong>{formatList(model.inputModalities, t)}</strong>
      </span>
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
    <div className="inline-model-editor">
      <div className="inline-model-heading">
        <div>
          <h3>{t("model.title")}</h3>
          <p>{t("model.defaultModelDesc")}</p>
        </div>
        <button disabled={disabled} onClick={onAddCustomModel}>
          <Plus size={16} />
          {t("model.customModelButton")}
        </button>
      </div>

      <div className="model-section-title">{t("model.defaultModels")}</div>
      <div className="model-list">
        {provider.defaultModels.map((model) => (
          <div className="model-row readonly detailed" key={model.slug}>
            <div className="model-row-title">
              <strong>{model.displayName}</strong>
              <span>{model.slug}</span>
            </div>
            {model.description ? <p>{model.description}</p> : null}
            <ModelParameterGrid model={model} t={t} />
          </div>
        ))}
      </div>

      <div className="model-section-title">{t("model.customModels")}</div>
      <div className="custom-model-list">
        {provider.customModels.length === 0 ? (
          <p className="muted">{t("model.noCustomModels")}</p>
        ) : (
          provider.customModels.map((model, index) => (
            <div className="custom-model-row detailed" key={`${model.slug}-${index}`}>
              <div className="custom-model-fields">
                <input
                  disabled={disabled}
                  value={model.slug}
                  onChange={(event) => onUpdateCustomModel(index, { slug: event.target.value })}
                  placeholder={t("model.slugPlaceholder")}
                />
                <input
                  disabled={disabled}
                  value={model.displayName}
                  onChange={(event) =>
                    onUpdateCustomModel(index, { displayName: event.target.value })
                  }
                  placeholder={t("model.displayNamePlaceholder")}
                />
                <input
                  disabled={disabled}
                  value={modelEffortsText(model)}
                  onChange={(event) =>
                    onUpdateCustomModel(index, {
                      reasoningEfforts: effortsFromText(event.target.value),
                    })
                  }
                  placeholder={t("model.effortsPlaceholder")}
                />
                <button
                  className="icon-button"
                  disabled={disabled}
                  onClick={() => onRemoveCustomModel(index)}
                  title={t("model.deleteTooltip")}
                >
                  <Trash2 size={16} />
                </button>
              </div>
              <ModelParameterGrid model={model} t={t} />
            </div>
          ))
        )}
      </div>
    </div>
  );
}
