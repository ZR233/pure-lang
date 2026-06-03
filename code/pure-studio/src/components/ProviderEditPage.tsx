import { ArrowLeft, CheckCircle2, Link2, Save, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ModelRecord, ProviderKind, ProviderRecord, ProviderTemplateRecord } from "../types";
import { allModels, providerStatusClass, translateStatus } from "../lib/utils";
import { ProviderModelEditor } from "./ProviderModelEditor";

type ProviderEditPageProps = {
  mode: "create" | "edit";
  provider: ProviderRecord;
  templates: ProviderTemplateRecord[];
  isSaving: boolean;
  onCancel: () => void;
  onSave: () => void;
  onChangeTemplate: (kind: ProviderKind) => void;
  onUpdateProvider: (updater: (provider: ProviderRecord) => ProviderRecord) => void;
  onAddCustomModel: () => void;
  onUpdateCustomModel: (index: number, patch: Partial<ModelRecord>) => void;
  onRemoveCustomModel: (index: number) => void;
};

export function ProviderEditPage({
  mode,
  provider,
  templates,
  isSaving,
  onCancel,
  onSave,
  onChangeTemplate,
  onUpdateProvider,
  onAddCustomModel,
  onUpdateCustomModel,
  onRemoveCustomModel,
}: ProviderEditPageProps) {
  const { t } = useTranslation();
  const models = allModels(provider);

  return (
    <section className="provider-edit-page">
      <header className="provider-edit-head">
        <button className="back-button" disabled={isSaving} onClick={onCancel}>
          <ArrowLeft size={18} />
        </button>
        <div className="provider-edit-title">
          <div className="provider-title-line">
            <h2>{mode === "create" ? t("provider.newProvider") : provider.name || provider.id}</h2>
            <span className={`provider-state ${providerStatusClass(provider)}`}>
              <CheckCircle2 size={14} />
              {translateStatus(provider.status, t)}
            </span>
          </div>
          <p>
            <Link2 size={14} />
            {provider.baseUrl || t("provider.defaultBaseUrl")}
          </p>
        </div>
        <div className="provider-edit-actions">
          <button disabled={isSaving} onClick={onCancel}>
            <X size={16} />
            {t("actions.cancel")}
          </button>
          <button className="primary" disabled={isSaving} onClick={onSave}>
            <Save size={16} />
            {t("actions.save")}
          </button>
        </div>
      </header>

      <div className="provider-edit-scroll">
        <div className="provider-form-grid">
          <label>
            <span>{t("provider.providerKey")}</span>
            <input
              disabled={isSaving}
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
            <span>{t("provider.providerType")}</span>
            <select
              disabled={isSaving}
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
            <span>{t("provider.displayName")}</span>
            <input
              disabled={isSaving}
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
            <span>{t("provider.protocolType")}</span>
            <span className="readonly-field">{provider.wireApi}</span>
          </label>
          <label className="wide">
            <span>{t("provider.baseUrl")}</span>
            <input
              disabled={isSaving}
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
            <span>{t("provider.apiKey")}</span>
            <input
              disabled={isSaving}
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
            <span>{t("provider.defaultModel")}</span>
            <select
              disabled={isSaving}
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
          disabled={isSaving}
          onAddCustomModel={onAddCustomModel}
          onUpdateCustomModel={onUpdateCustomModel}
          onRemoveCustomModel={onRemoveCustomModel}
        />
      </div>
    </section>
  );
}
