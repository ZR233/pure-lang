import { Plus, Trash2 } from "lucide-react";
import type { ModelRecord, ProviderRecord } from "../types";

type ProviderModelEditorProps = {
  provider: ProviderRecord;
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

function formatTokens(value?: number | null) {
  if (value === undefined || value === null) {
    return "未配置";
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

function formatList(values?: string[]) {
  return values && values.length > 0 ? values.join(", ") : "未配置";
}

function ModelParameterGrid({ model }: { model: ModelRecord }) {
  return (
    <div className="model-parameter-grid">
      <span>
        <small>Context</small>
        <strong>{formatTokens(model.contextWindow ?? model.maxContextWindow)}</strong>
      </span>
      <span>
        <small>Max output</small>
        <strong>{formatTokens(model.maxOutputTokens)}</strong>
      </span>
      <span>
        <small>Auto compact</small>
        <strong>{formatTokens(model.autoCompactTokenLimit)}</strong>
      </span>
      <span>
        <small>Temperature</small>
        <strong>{model.defaultTemperature ?? "未配置"}</strong>
      </span>
      <span>
        <small>Efforts</small>
        <strong>{modelEffortsText(model) || "未配置"}</strong>
      </span>
      <span>
        <small>Truncation</small>
        <strong>
          {model.truncationMode ?? "tokens"} / {formatTokens(model.truncationLimit)}
        </strong>
      </span>
      <span className="wide">
        <small>Capabilities</small>
        <strong>{formatList(model.capabilities)}</strong>
      </span>
      <span className="wide">
        <small>Input</small>
        <strong>{formatList(model.inputModalities)}</strong>
      </span>
    </div>
  );
}

export function ProviderModelEditor({
  provider,
  onAddCustomModel,
  onUpdateCustomModel,
  onRemoveCustomModel,
}: ProviderModelEditorProps) {
  return (
    <div className="inline-model-editor">
      <div className="inline-model-heading">
        <div>
          <h3>Models</h3>
          <p>默认模型由模板提供，自定义模型追加在后面。</p>
        </div>
        <button onClick={onAddCustomModel}>
          <Plus size={16} />
          Custom Model
        </button>
      </div>

      <div className="model-section-title">Default Models</div>
      <div className="model-list">
        {provider.defaultModels.map((model) => (
          <div className="model-row readonly detailed" key={model.slug}>
            <div className="model-row-title">
              <strong>{model.displayName}</strong>
              <span>{model.slug}</span>
            </div>
            {model.description ? <p>{model.description}</p> : null}
            <ModelParameterGrid model={model} />
          </div>
        ))}
      </div>

      <div className="model-section-title">Custom Models</div>
      <div className="custom-model-list">
        {provider.customModels.length === 0 ? (
          <p className="muted">No custom models.</p>
        ) : (
          provider.customModels.map((model, index) => (
            <div className="custom-model-row detailed" key={`${model.slug}-${index}`}>
              <div className="custom-model-fields">
                <input
                  value={model.slug}
                  onChange={(event) => onUpdateCustomModel(index, { slug: event.target.value })}
                  placeholder="model-slug"
                />
                <input
                  value={model.displayName}
                  onChange={(event) =>
                    onUpdateCustomModel(index, { displayName: event.target.value })
                  }
                  placeholder="Display name"
                />
                <input
                  value={modelEffortsText(model)}
                  onChange={(event) =>
                    onUpdateCustomModel(index, {
                      reasoningEfforts: effortsFromText(event.target.value),
                    })
                  }
                  placeholder="high, xhigh"
                />
                <button
                  className="icon-button"
                  onClick={() => onRemoveCustomModel(index)}
                  title="删除自定义模型"
                >
                  <Trash2 size={16} />
                </button>
              </div>
              <ModelParameterGrid model={model} />
            </div>
          ))
        )}
      </div>
    </div>
  );
}
