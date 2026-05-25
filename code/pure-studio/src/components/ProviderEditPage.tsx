import { ArrowLeft, CheckCircle2, Link2 } from "lucide-react";
import type { ModelRecord, ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";
import { ProviderModelEditor } from "./ProviderModelEditor";

type ProviderEditPageProps = {
  provider: ProviderRecord;
  templates: ProviderTemplateRecord[];
  onBack: () => void;
  onChangeTemplate: (kind: ProviderKind) => void;
  onUpdateProvider: (updater: (provider: ProviderRecord) => ProviderRecord) => void;
  onAddCustomModel: () => void;
  onUpdateCustomModel: (index: number, patch: Partial<ModelRecord>) => void;
  onRemoveCustomModel: (index: number) => void;
};

function allModels(provider: ProviderRecord) {
  return [...provider.defaultModels, ...provider.customModels];
}

function providerStatusClass(provider: ProviderRecord) {
  return provider.status.toLowerCase().includes("healthy") ? "ready" : "needs-setup";
}

export function ProviderEditPage({
  provider,
  templates,
  onBack,
  onChangeTemplate,
  onUpdateProvider,
  onAddCustomModel,
  onUpdateCustomModel,
  onRemoveCustomModel,
}: ProviderEditPageProps) {
  const models = allModels(provider);

  return (
    <section className="provider-edit-page">
      <header className="provider-edit-head">
        <button className="back-button" onClick={onBack}>
          <ArrowLeft size={18} />
        </button>
        <div className="provider-edit-title">
          <div className="provider-title-line">
            <h2>{provider.name || provider.id}</h2>
            <span className={`provider-state ${providerStatusClass(provider)}`}>
              <CheckCircle2 size={14} />
              {provider.status}
            </span>
          </div>
          <p>
            <Link2 size={14} />
            {provider.baseUrl || "(default base URL)"}
          </p>
        </div>
      </header>

      <div className="provider-edit-scroll">
        <div className="provider-form-grid">
          <label>
            <span>Provider Key</span>
            <input
              value={provider.id}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  id: event.target.value,
                }))
              }
            />
          </label>
          <label>
            <span>Template</span>
            <select
              value={provider.templateKind}
              onChange={(event) => onChangeTemplate(event.target.value as ProviderKind)}
            >
              {templates.map((template) => (
                <option key={template.id} value={template.id}>
                  {template.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            <span>显示名</span>
            <input
              value={provider.name}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  name: event.target.value,
                }))
              }
            />
          </label>
          <label>
            <span>协议类型</span>
            <span className="readonly-field">{provider.wireApi}</span>
          </label>
          <label className="wide">
            <span>Base URL</span>
            <input
              value={provider.baseUrl}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  baseUrl: event.target.value,
                }))
              }
            />
          </label>
          <label>
            <span>API Key</span>
            <input
              type="password"
              value={provider.bearerToken}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  bearerToken: event.target.value,
                }))
              }
            />
          </label>
          <label className="wide">
            <span>Default Model</span>
            <select
              value={provider.defaultModel}
              onChange={(event) =>
                onUpdateProvider((current) => ({
                  ...current,
                  defaultModel: event.target.value,
                }))
              }
            >
              {models.map((model) => (
                <option key={model.slug} value={model.slug}>
                  {model.displayName} ({model.slug})
                </option>
              ))}
            </select>
          </label>
        </div>

        <ProviderModelEditor
          provider={provider}
          onAddCustomModel={onAddCustomModel}
          onUpdateCustomModel={onUpdateCustomModel}
          onRemoveCustomModel={onRemoveCustomModel}
        />
      </div>
    </section>
  );
}
